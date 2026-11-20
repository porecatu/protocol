# 02 — Handshake e pareamento

**Versão da especificação:** 0.5 (rascunho)

Este documento cobre a camada 3: as chaves de cada parte, o pareamento por QR code, o handshake das conexões seguintes e o formato dos registros cifrados. A escolha do Noise está no [ADR-0002](../adr/0002-noise-para-a-sessao-ponta-a-ponta.md).

## 1. Chaves

| Chave | Tipo | De quem | Para quê | Onde mora |
|---|---|---|---|---|
| Estática do host | X25519 | host | Identidade do host na sessão Noise; vai no QR | Cofre do sistema operacional do desktop |
| De roteamento do host | Ed25519 | host | Provar ao relay a posse do `host_id` ([01](01-transporte-e-relay.md) §3) | Cofre do sistema operacional do desktop |
| Estática do aparelho | X25519 | cliente | Identidade do aparelho na sessão Noise | Armazenamento seguro do celular (Keychain no iOS, Keystore no Android) |
| Efêmeras | X25519 | os dois | Sigilo futuro de cada sessão | Memória, descartadas no fim do handshake |

- As chaves são geradas por gerador criptográfico do sistema, na primeira vez que são necessárias, e **nunca** saem do aparelho que as gerou, exceto as partes públicas.
- A chave estática do host e a de roteamento são **pares de chaves distintos** (uma não é derivada da outra), mas têm **o mesmo ciclo de vida**: o host **NÃO DEVE** trocar uma sem trocar a outra. Trocar a estática **desfaz todos os pareamentos** (os aparelhos guardaram a pública antiga); trocar a de roteamento **muda o `host_id`**, que o QR também carregou. Como as duas trocas invalidam os mesmos pareamentos, elas acontecem juntas, num gesto só de "recriar identidade" no host.

### Impressão digital

Para exibir uma chave pública a um humano (no diálogo de confirmação do host e na tela do aparelho):

```
fingerprint(pub) = base32_maiúsculo_sem_padding( BLAKE2s-256(pub)[0..10] )   ; 16 caracteres
exibição         = quatro grupos de quatro, separados por espaço: "ABCD EFGH IJKL MNOP"
```

## 2. Protocolos Noise

| Uso | Nome Noise | Iniciador |
|---|---|---|
| Pareamento (aparelho novo) | `Noise_XX_25519_ChaChaPoly_BLAKE2s` | cliente |
| Conexão de aparelho pareado | `Noise_IK_25519_ChaChaPoly_BLAKE2s` | cliente |

**Prólogo** dos dois: os bytes ASCII de `"porecatu-remote/0 "` seguidos do `host_id` (ex.: `porecatu-remote/0 k3x9...`). O prólogo amarra a sessão ao endereço pelo qual o cliente chegou e à versão maior do protocolo; prólogos diferentes fazem o handshake falhar no primeiro `es`/`ee`.

O primeiro registro que o cliente manda num canal diz qual dos dois handshakes vem a seguir:

```cddl
handshake-start = [kind: &(pair: 0, connect: 1), noise-message-1: bstr]
```

Esse item CBOR é o primeiro `payload` do canal (ainda em claro: carrega só a mensagem 1 do Noise, que já é o que o padrão expõe). Todos os `payload` seguintes são mensagens Noise cruas, de handshake ou de transporte.

O handshake **DEVE** terminar em 15 s contados do `channel-opened`; o host fecha o canal que estourar com `close-channel { host-rejected }`.

**Falha de handshake.** Qualquer falha do lado do host — mensagem Noise que não decifra (inclusive por prólogo de `major` diferente), `handshake-start` mal formado, tipo de handshake desconhecido — fecha o canal com `close-channel { host-rejected }`, sem mensagem de aplicação. O cliente vê `closed { host-rejected }` e trata como falha de handshake. Do lado do cliente, falha de decifração da `msg2` aborta sem mandar nada mais.

**Resultado de DH nulo.** As duas partes **DEVEM** recusar, como falha de handshake, qualquer operação DH cujo resultado seja 32 bytes zero (chave pública de ordem baixa). As bibliotecas diferem nisso por padrão; a regra é do protocolo, e há vetor de teste para ela.

## 3. Pareamento

### 3.1 O QR code

O host gera, a pedido do usuário, um **token de pareamento**: 16 bytes aleatórios, **de uso único**, válido por **5 minutos**. Há no máximo um token vivo por host: gerar outro invalida o anterior, e fechar a tela que mostra o QR também.

O QR codifica uma URI:

```
porecatu://pair?v=0&relay=<url>&host=<host_id>&key=<pub>&token=<token>&name=<nome>
```

| Parâmetro | Conteúdo |
|---|---|
| `v` | Versão maior do protocolo (`0`) |
| `relay` | URL base do relay, percent-encoded (ex.: `wss%3A%2F%2Frelay.exemplo.com.br`) |
| `host` | `host_id` ([01](01-transporte-e-relay.md) §3) |
| `key` | Chave pública estática do host, base64url sem padding (43 caracteres) |
| `token` | Token de pareamento, base64url sem padding (22 caracteres) |
| `name` | Nome do host para exibição (padrão: nome da máquina), UTF-8 percent-encoded, de 1 a 32 valores escalares Unicode |

O esquema `porecatu://` permite que a câmera do sistema abra o app direto. O cliente **DEVE** recusar URI com `v` diferente da sua versão maior, ou com `relay` que não seja `wss://` (salvo `localhost`, como em [01](01-transporte-e-relay.md) §1).

**Forma exata da URI**, para as duas implementações gerarem os mesmos bytes:

- Os parâmetros saem **nesta ordem**: `v`, `relay`, `host`, `key`, `token`, `name`.
- Todo byte UTF-8 fora do conjunto *unreserved* da RFC 3986 (`A–Z a–z 0–9 - . _ ~`) é escrito como `%HH`, com **hexadecimal maiúsculo**. Nada mais é deixado em claro.
- `relay` é normalizado antes de codificar: esquema e host em minúsculas, sem barra final, sem query nem fragmento; pode ter porta e um prefixo de caminho (relay atrás de proxy num subcaminho). Hosts locais aceitos com `ws://`: `localhost`, `127.0.0.1` e `[::1]`.
- Quem lê **ignora parâmetros desconhecidos** (versões menores futuras) e **recusa** parâmetro obrigatório ausente, repetido ou mal formado, e `key`/`token` com tamanho decodificado diferente de 32/16 bytes.

O **token do operador do relay não vai no QR** ([01](01-transporte-e-relay.md) §2).

### 3.2 O handshake `XX`

```
cliente -> host:  [0, msg1]            msg1 = -> e
host -> cliente:  msg2                 msg2 = <- e, ee, s, es    payload: vazio
cliente -> host:  msg3                 msg3 = -> s, se           payload: pair-request
```

1. Ao receber a `msg2`, o cliente **DEVE** comparar a chave estática que o host revelou com a `key` do QR, em tempo constante. Diferente: aborta sem mandar a `msg3` e mostra erro de identidade. É isso que impede um relay malicioso de se passar pelo host.
2. A `msg3` leva, cifrado, o pedido:

   ```cddl
   pair-request = {
     "token":    bstr .size 16,
     "device":   tstr .size (1..64),   ; nome do aparelho, escolhido pelo usuário (padrão: modelo)
     "platform": &(android: 0, ios: 1, other: 2),
   }
   ```

3. O host confere o token: existe, não expirou, não foi usado. A conferência **DEVE** ser em tempo constante e **consome** o token mesmo quando o resto falha. Token inválido: o host manda a mensagem `pair-rejected { reason: token-invalid }` ([03](03-mensagens.md) §8) e fecha o canal.
4. Token válido: o host mostra ao usuário um **diálogo de confirmação** com o nome do aparelho e a impressão digital da chave estática dele (§1). O cliente mostra a mesma impressão digital na tela, para comparação. O diálogo espera até **120 s**.
5. O usuário confirma: o host grava o aparelho na lista de pareados (chave pública, nome, plataforma, data) e manda `pair-accepted`. A sessão **continua** como uma conexão normal, a partir do `hello` ([03](03-mensagens.md) §2): não há reconexão.
6. O usuário recusa, ou os 120 s passam: `pair-rejected { reason: user-declined | timeout }` e o canal fecha.

Um aparelho cuja chave já está na lista e pareia de novo **substitui** a entrada (nome e data novos), sem duplicar.

## 4. Conexão de aparelho pareado: `IK`

```
cliente -> host:  [1, msg1]            msg1 = -> e, es, s, ss    payload: vazio
host -> cliente:  msg2                 msg2 = <- e, ee, se       payload: vazio
```

- O cliente conhece a estática do host desde o QR; é isso que permite o `IK`, com um round-trip só.
- O host lê a estática do cliente na `msg1` e **sempre completa o handshake**, pareado ou não. Se o aparelho não está na lista (nunca pareou, ou foi revogado), a primeira mensagem de aplicação do host é `error { code: not-paired }` e o canal fecha. Completar o handshake antes de recusar dá ao cliente uma resposta autenticada ("este aparelho não está pareado") sem revelar nada a quem não tem a chave do host.
- Os payloads do handshake vão vazios de propósito: o da `msg1` do `IK` não tem sigilo futuro. O receptor **DEVE** recusar, como falha de handshake, uma `msg1` (`XX` ou `IK`) ou `msg2` cujo payload decifrado não seja vazio. Só a `msg3` do `XX` leva payload (o `pair-request`).

## 5. Registros

Depois do handshake, cada `payload` do canal é **uma mensagem de transporte Noise**: no máximo 65 535 bytes de texto cifrado, portanto no máximo 65 519 bytes de texto claro.

O texto claro de cada registro é:

```
+--------+---------------------------------------------+
| flags  | fragmento da mensagem CBOR                  |
| 1 byte | 0 .. 65 518 bytes                           |
+--------+---------------------------------------------+

flags bit 0  (0x01)  CONTINUA   a mensagem segue no próximo registro
flags bit 1  (0x02)  REKEY      o remetente chama Rekey() na sua cifra de envio depois deste registro
bits 2..7            reservados; DEVEM ser zero, e registro com bit reservado ligado fecha a sessão
```

- Uma **mensagem** ([03](03-mensagens.md)) é a concatenação dos fragmentos de registros consecutivos até um com `CONTINUA` desligado. Mensagens não se intercalam: o remetente termina uma antes de começar outra.
- Mensagem remontada maior que **16 MiB** fecha a sessão.
- **Rekey**: cada lado **DEVE** marcar `REKEY` num registro e trocar a chave de envio pelo menos a cada 100 000 registros enviados ou a cada 60 minutos, o que vier antes. Quem recebe um registro com `REKEY` troca a chave de recebimento depois de decifrá-lo. Como os registros chegam em ordem ([01](01-transporte-e-relay.md) §4), os dois lados ficam sincronizados.
- Falha de decifração **DEVE** fechar a sessão: não há reenvio nem salto de nonce.

## 6. Revogação

- O usuário remove um aparelho no host: a entrada sai da lista e **toda sessão viva** daquele aparelho recebe `goodbye { reason: revoked }` ([03](03-mensagens.md) §2) e tem o canal fechado. Uma tentativa de conexão **posterior** do mesmo aparelho recebe `error { code: not-paired }` (§4): para o host, revogado e nunca pareado são o mesmo estado.
- Desligar o acesso remoto no host fecha todos os canais e a conexão com o relay; a lista de pareados fica.
- O cliente remove um host da lista dele: apaga a chave pública guardada; o host só descobre na próxima vez em que o usuário o limpar da lista. Não há mensagem de "despareamento" iniciada pelo cliente no v0.
