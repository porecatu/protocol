# ADR-0004 — Transporte por WebSocket sobre TLS, quadros binários, um envelope por quadro

**Status:** Aceito
**Data:** 2026-11-15
**Relacionados:** [ADR-0001](0001-tres-partes-e-relay-cego.md), [ADR-0003](0003-cbor-com-schema-em-cddl.md), [01 — Transporte e relay](../especificacao/01-transporte-e-relay.md)

## Contexto

Host e cliente conectam ao relay, nunca um ao outro ([ADR-0001](0001-tres-partes-e-relay-cego.md)). O transporte até o relay precisa:

- **atravessar redes hostis**: Wi-Fi de hotel, rede corporativa com proxy, operadora móvel com NAT de operadora; na prática, só a porta 443 é garantida;
- **ser suportado nos três lados**: Rust com `tokio` (relay e host) e React Native/Expo (cliente), este sem módulo nativo se possível;
- **encaixar no auto-hospedado comum**: o operador provavelmente já tem um proxy reverso (Caddy, nginx, Traefik) emitindo certificados;
- **preservar ordem e integridade** dos registros Noise: um registro perdido ou reordenado quebra o contador de nonce e a sessão inteira.

## Decisão

**Host e cliente conectam ao relay por WebSocket sobre TLS (`wss://`), só com quadros binários, cada quadro carregando exatamente um envelope CBOR. O keepalive é o Ping/Pong do WebSocket, iniciado pelo relay.**

1. TLS 1.2 ou superior (1.3 recomendado); `ws://` só para `localhost`.
2. Dois caminhos: `/v0/host` (com token do operador no `Authorization` do upgrade) e `/v0/client/<host_id>`. A versão no caminho é a do envelope.
3. Teto de 70 000 bytes por quadro (um registro Noise máximo mais envelope); acima disso, fechamento com `1009`.
4. Ping a cada 25 s, conexão morta em 60 s sem Pong.
5. Multiplexação de clientes **dentro** da conexão do host, por `channel` escolhido pelo relay: o host mantém uma conexão só, não uma por celular.
6. Códigos de fechamento próprios na faixa `4000–4999` ([01](../especificacao/01-transporte-e-relay.md) §5).

## Alternativas consideradas

- **QUIC / HTTP/3 (ou WebTransport).** Melhor em rede móvel (migração de conexão, sem bloqueio de cabeça de fila). Rejeitada no v0: UDP é bloqueado em muitas redes corporativas, o suporte no React Native é imaturo, e proxies reversos comuns ainda não o encaminham de forma transparente. Pode entrar como transporte alternativo num envelope futuro.
- **TCP cru com TLS, porta própria.** Mais simples que WebSocket. Rejeitada: porta não-padrão é bloqueada, não passa por proxy reverso HTTP, e o React Native não tem socket TCP sem módulo nativo.
- **gRPC (HTTP/2) com streaming bidirecional.** Tipado e multiplexado. Rejeitada: no React Native gRPC exige ponte nativa ou gRPC-Web (que não tem streaming bidirecional), e o relay não precisa de RPC tipado — só encaminha bytes opacos.
- **HTTP long-polling ou Server-Sent Events.** Funciona em qualquer lugar. Rejeitada pela latência de digitação e pelo custo de reabrir requisições.
- **Uma conexão WebSocket do host por cliente.** Isolamento por conexão. Rejeitada: o relay teria de avisar o host de cada cliente novo por outro canal de qualquer forma, e o host abriria conexões sob demanda; o canal multiplexado resolve com uma conexão só.

## Consequências

### Positivas

- Funciona atrás de proxy reverso existente e na porta 443, sem configuração de rede pelo usuário.
- API `WebSocket` nativa no React Native: o cliente não precisa de módulo nativo para o transporte.
- Ordem e integridade garantidas pelo TCP, que é o que o Noise exige.

### Negativas

- Bloqueio de cabeça de fila: um quadro grande de grade atrasa uma tecla atrás dele na mesma conexão. Mitigado pelo controle de fluxo da grade ([03](../especificacao/03-mensagens.md) §5.4), não eliminado.
- Troca de rede no celular (Wi-Fi → 4G) derruba a conexão; o cliente reconecta e refaz o `IK`, e reassina os painéis.
- Overhead pequeno de cabeçalho por quadro e do Ping periódico.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Proxy reverso com timeout ocioso menor que o Ping | Média | Baixo | Ping a cada 25 s, abaixo dos 60 s típicos; documentação de operação no Relay |
| Proxy que limita tamanho de quadro WebSocket | Baixa | Médio | Quadro máximo de 70 000 bytes, documentado como requisito do proxy |
| Reconexões frequentes em rede móvel | Alta | Baixo | `IK` com um round-trip e reassinatura com snapshot: a reconexão é barata |
