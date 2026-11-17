# 01 — Transporte e relay

**Versão da especificação:** 0.4 (rascunho)

Este documento cobre as camadas 1 e 2: como host e cliente chegam ao relay e o que o relay vê. Tudo acima da camada 2 é opaco para o relay.

## 1. Transporte

- Host e cliente **DEVEM** conectar ao relay por **WebSocket sobre TLS** (`wss://`), TLS 1.2 ou superior; TLS 1.3 **DEVERIA** ser o único habilitado em instalação nova. `ws://` sem TLS **NÃO DEVE** ser aceito pelas implementações de host e cliente, exceto quando o endereço do relay é `localhost`/`127.0.0.1`/`::1` (desenvolvimento).
- Só **quadros binários** são usados. Quadro de texto recebido **DEVE** fechar a conexão com o código `1003`.
- Cada quadro binário WebSocket carrega **exatamente um envelope** (§4). O tamanho máximo de um quadro é **70 000 bytes** (um registro Noise de 65 535 bytes mais o envelope, com folga); quadro maior **DEVE** fechar a conexão com `1009`.
- Keepalive: o relay **DEVE** mandar `Ping` WebSocket a cada 25 s e fechar a conexão que não responder em 60 s. Host e cliente **DEVEM** responder `Pong` (as bibliotecas de WebSocket já fazem isso). É o único keepalive: não há mensagem de aplicação para isso.
- Por que WebSocket e não QUIC ou TCP cru: passa por proxy reverso, CDN e firewall corporativo na porta 443 sem configuração, e há implementação madura em Rust (`tokio-tungstenite`) e no React Native (API `WebSocket` nativa). A decisão está no [ADR-0004](../adr/0004-websocket-sobre-tls.md).

## 2. Endpoints

O relay expõe dois caminhos, sob a URL base configurada pelo usuário (ex.: `wss://relay.exemplo.com.br`):

| Caminho | Quem conecta | Autenticação na abertura |
|---|---|---|
| `/v0/host` | host | Cabeçalho `Authorization: Bearer <token do operador>`; depois, desafio da chave de roteamento (§3) |
| `/v0/client/<host_id>` | cliente | Nenhuma no relay. A autenticação do cliente é a sessão Noise, feita com o host (camada 3) |

E um terceiro, fora do WebSocket:

| Caminho | Quem consulta | Resposta |
|---|---|---|
| `GET /v0/presence/<host_id>` (HTTPS) | cliente | `204` se há host registrado com esse `host_id`, `404` se não. Sem corpo. Sujeito ao limite de consultas por origem (`429`) |

A URL da consulta é **derivada da URL base do relay**: `wss://` vira `https://` e `ws://` vira `http://`, com a mesma autoridade e o mesmo prefixo de caminho, seguidos de `/v0/presence/<host_id>`. Ex.: `wss://exemplo.com.br/relay` → `https://exemplo.com.br/relay/v0/presence/<host_id>`.

A consulta de presença existe para o cliente mostrar quais desktops estão online **sem abrir canal**: abrir um canal só para sondar dispararia `channel-opened` e o prazo de handshake no host. Ela revela a quem conhece o `host_id` se o desktop está ligado agora; ver o [modelo de ameaça](modelo-de-ameaca.md).

O prefixo `/v0/` é a versão do **envelope do relay**, independente da versão das mensagens de aplicação ([04](04-versionamento.md)).

### Token do operador

O relay é auto-hospedado e **não tem contas**. Para não virar relay aberto a qualquer um, o operador define um ou mais tokens na configuração do relay, e só um host que apresente um deles consegue se registrar. O token **não** protege o conteúdo (quem faz isso é o Noise); protege o servidor do operador contra uso por terceiros.

- O token é uma string opaca de 32 a 128 caracteres ASCII imprimíveis. O relay **DEVE** compará-lo em tempo constante.
- Token ausente ou errado: o relay responde **HTTP 401** antes do upgrade e não abre WebSocket.
- O cliente **nunca** recebe nem precisa do token: o QR do pareamento não o carrega.

## 3. Endereço do host (`host_id`) e registro

O host tem uma **chave de roteamento** Ed25519, separada da chave estática Noise (que é X25519; ver [02](02-handshake-e-pareamento.md) §1). Ela existe só para provar ao relay que quem registra um `host_id` é o dono dele.

```
host_id = base32_sem_padding_minúsculo( BLAKE2s-256( chave_pública_de_roteamento )[0..20] )
```

São 20 bytes (160 bits), 32 caracteres no alfabeto `abcdefghijklmnopqrstuvwxyz234567` (RFC 4648, minúsculo, sem `=`).

Registro, logo depois do upgrade em `/v0/host`:

1. Relay → host: `Challenge { nonce: 32 bytes aleatórios }`.
2. Host → relay: `Register { routing_key: 32 bytes, signature: 64 bytes }`, com `signature = Ed25519_sign(chave_privada_de_roteamento, "porecatu-relay/v0/register" || nonce)`.
3. O relay verifica a assinatura e calcula o `host_id`. Se ok: `Registered { host_id }`. Se não: fecha com o código `4001`. Se o relay já tem o número máximo de hosts que o operador configurou e este `host_id` não está entre eles (substituição não conta), fecha com `4003`.

O host tem **10 s** desde o upgrade para concluir o registro; passado o prazo, o relay fecha com `4005`.

**A última registração válida vence.** Se já há um host registrado com o mesmo `host_id` (o mesmo desktop que perdeu a rede e voltou), o relay fecha a conexão antiga com `4002`, e **cada cliente** dos canais dela recebe `closed { host-replaced }` e o fechamento do seu WebSocket. Ninguém sem a chave privada consegue tomar o endereço, então isso não abre sequestro.

## 4. Envelope do relay

Todo quadro binário entre host e relay, e entre cliente e relay, é **um item CBOR** (RFC 8949, codificação determinística da §4.2.1: inteiros e comprimentos na forma mais curta, mapas com chaves ordenadas). Os envelopes são **arrays** cujo primeiro elemento é o tipo.

```cddl
; ---- host <-> relay -----------------------------------------------------
host-envelope = challenge / register / registered
              / channel-opened / channel-data / channel-closed / close-channel

challenge      = [0, nonce: bstr .size 32]                         ; relay -> host
register       = [1, routing-key: bstr .size 32, signature: bstr .size 64]  ; host -> relay
registered     = [2, host-id: tstr]                                ; relay -> host
channel-opened = [3, channel: uint]                                ; relay -> host
channel-data   = [4, channel: uint, payload: bstr]                 ; nos dois sentidos
channel-closed = [5, channel: uint, reason: close-reason]          ; relay -> host
close-channel  = [6, channel: uint, reason: close-reason]          ; host -> relay

; ---- cliente <-> relay --------------------------------------------------
client-envelope = data / host-unavailable / closed

data             = [16, payload: bstr]                             ; nos dois sentidos
host-unavailable = [17]                                            ; relay -> cliente
closed           = [18, reason: close-reason]                      ; relay -> cliente

close-reason = &(
  normal:           0,   ; o outro lado fechou
  host-replaced:    1,   ; o host se registrou de novo
  host-gone:        2,   ; a conexão do host caiu
  host-rejected:    3,   ; o host recusou o canal (ex.: acesso remoto desligado)
  limit-exceeded:   4,   ; limite do relay (canais por host, banda, tamanho)
  protocol-error:   5,   ; envelope inválido
)
```

`payload` é **opaco** para o relay: é um registro Noise (camada 3), e o relay **NÃO DEVE** tentar interpretá-lo, guardá-lo ou registrá-lo em log.

### Ciclo de um canal

1. Cliente abre `wss://<relay>/v0/client/<host_id>`.
2. Se o `host_id` não tem host registrado, o relay manda `host-unavailable` e fecha com `1000`. O cliente mostra o host como **offline**.
3. Se tem, o relay escolhe um `channel` (inteiro não negativo, **único entre os canais vivos daquele host**, nunca reaproveitado durante a vida da conexão do host) e manda `channel-opened` ao host.
4. Daí em diante, cada `data` do cliente vira `channel-data` para o host com o mesmo `payload`, e vice-versa, **na ordem em que chegaram**.
5. Qualquer lado fecha: o cliente fechando o WebSocket gera `channel-closed { normal }` ao host; o host mandando `close-channel` gera `closed` ao cliente e o fechamento do WebSocket dele.

6. A conexão do host cai: cada cliente dos canais dela recebe `closed { host-gone }` e o fechamento do seu WebSocket.

O host **PODE** fechar um canal a qualquer momento (acesso remoto desligado, aparelho revogado, handshake inválido). O relay **NÃO DEVE** reabrir um canal fechado.

- `channel-data` ou `close-channel` que o host mande para um canal **já fechado** (corrida com o fechamento pelo cliente) é **ignorado** pelo relay, sem erro: o `channel-closed` correspondente já está a caminho do host. Para um canal que **nunca foi aberto** (número maior ou igual ao próximo que o relay daria), é envelope inválido: `4004` (abaixo).
- O relay **não** devolve `channel-closed` em resposta a um `close-channel` do host.
- **Erros de envelope.** Do **host**, depois do registro: envelope que não decodifica, fora da forma canônica ([03](03-mensagens.md) §1.1, que vale também aqui) ou de tipo que o host não manda → o relay fecha a conexão do host com `4004`, e cada cliente dela recebe `closed { host-gone }`. Do **cliente**: qualquer coisa que não seja `data` válido → `closed { protocol-error }` e `4004` ao cliente, e `channel-closed { protocol-error }` ao host. Quadro de texto WebSocket fecha com `1003`, sem `closed` antes.
- **Limites estourados por um canal** (fila, banda, tamanho): `closed { limit-exceeded }` e `4003` ao cliente, `channel-closed { limit-exceeded }` ao host. Um `data` do cliente cujo `channel-data` correspondente passaria do tamanho máximo de quadro é limite estourado. Quadro WebSocket maior que o máximo, de qualquer lado, fecha com `1009`.
- **Pong que não chega** (§1): o relay derruba a conexão sem código de fechamento; se era um host, os clientes recebem `closed { host-gone }`; se era um cliente, o host recebe `channel-closed { normal }`.
- **Desligamento do relay**: clientes recebem `closed { normal }`; todas as conexões fecham com `1001`.
- Depois de mandar `closed { reason }` ao cliente, o relay fecha o WebSocket dele com `4003` se o motivo é `limit-exceeded`, `4004` se é `protocol-error`, e `1000` nos demais.
- Erros antes do upgrade são respostas HTTP: `400` para `host_id` mal formado em `/v0/client/<host_id>`, `401` para token do operador ausente ou errado, `429` quando o relay recusa por excesso de conexões da mesma origem.

## 5. Códigos de fechamento WebSocket

Além dos padronizados (`1000`, `1001`, `1003`, `1008`, `1009`, `1011`):

| Código | Significado |
|---|---|
| `4001` | Registro do host recusado (assinatura inválida) |
| `4002` | Substituído por um registro mais novo do mesmo `host_id` |
| `4003` | Limite do relay excedido |
| `4004` | Envelope inválido |
| `4005` | Registro do host não concluído no prazo (§3) |

## 6. Limites mínimos que o relay DEVE suportar

Os limites são configuráveis pelo operador; estes são os pisos que uma implementação conforme garante, para o host e o cliente poderem contar com eles.

| Limite | Piso |
|---|---|
| Canais simultâneos por host | 8 |
| Tamanho de quadro | 70 000 bytes (§1) — é piso **e** teto: nenhum lado manda quadro maior |
| Banda por canal, sustentada, **em cada sentido** | 2 Mbit/s |

Acima do limite, o relay fecha o canal com `limit-exceeded`, nunca descarta um `payload` em silêncio: um registro Noise perdido quebra o contador de nonce e a sessão inteira.

## 7. O que o relay vê

Para o modelo de ameaça ([modelo-de-ameaca.md](modelo-de-ameaca.md)), o relay sabe, e só sabe:

- o endereço IP de host e clientes e os horários de conexão;
- o `host_id` e a chave pública de roteamento do host;
- quantos canais cada host tem abertos;
- o tamanho e o momento de cada `payload`.

Não sabe: quem é o aparelho (a chave estática do cliente viaja cifrada no handshake `XX` e `IK`), o que há nos terminais, as teclas, os nomes de abas, diretórios ou perfis.
