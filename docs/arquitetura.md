# Arquitetura

Como o protocolo se organiza em camadas e como as implementações de referência ([ADR-0005](adr/0005-implementacoes-de-referencia.md)) se encaixam nos três projetos que o falam. A especificação diz **o quê**; este documento diz **onde cada coisa mora**.

**Estado:** só a especificação existe (P0 do [roadmap](roadmap.md)). Tudo o que fala de `rust/` e `ts/` é o desenho para a P1 e a P2.

## 1. Camadas e quem as implementa

```
               HOST (Porecatu)            RELAY                  CLIENTE (Remote)
               Rust                       Rust                   TypeScript (Expo)
             +-----------------+                               +-----------------+
 camada 4    | mensagens       |                               | mensagens       |
 (CBOR)      | workspace, grade|                               | workspace, grade|
             +-----------------+                               +-----------------+
 camada 3    | Noise XX / IK   |                               | Noise XX / IK   |
 (registros) | registros, rekey|                               | registros, rekey|
             +-----------------+      +-----------------+      +-----------------+
 camada 2    | envelope host   |<---->| envelope host + |<---->| envelope cliente|
 (envelope)  | (canais)        |      | cliente, canais |      |                 |
             +-----------------+      +-----------------+      +-----------------+
 camada 1    | WebSocket + TLS |<---->| WebSocket + TLS |<---->| WebSocket + TLS |
             +-----------------+      +-----------------+      +-----------------+
                 consumidor               consumidor               consumidor
                 (porecatu-remote)        (relay)                  (app)

 Implementado AQUI: camadas 2, 3 e 4.   Implementado nos consumidores: camada 1.
```

| Camada | Especificação | Crate Rust | Pacote TS |
|---|---|---|---|
| 1 — WebSocket/TLS | [01](especificacao/01-transporte-e-relay.md) §1 | — (consumidor) | — (consumidor) |
| 2 — Envelope do relay | [01](especificacao/01-transporte-e-relay.md) §2–§5 | módulo `envelope` (feature `envelope`, a única que o Relay liga) | módulo `envelope` (só o lado do cliente) |
| 3 — Noise e registros | [02](especificacao/02-handshake-e-pareamento.md) | módulo `session` (sobre `snow`) | módulo `session` (sobre `@noble`) |
| 4 — Mensagens | [03](especificacao/03-mensagens.md) | módulo `message` (`serde` + `ciborium`) | módulo `message` (`cbor-x`) |

## 2. O que as implementações de referência são, e o que não são

São **bibliotecas puras**: recebem bytes e o instante atual, devolvem bytes e eventos. Não abrem socket, não criam thread, não leem relógio, não guardam chave em disco.

| Responsabilidade | Onde fica |
|---|---|
| Codificar e decodificar envelope e mensagens | aqui |
| Máquina de estado do handshake (`XX`, `IK`), registros, fragmentação, rekey | aqui |
| Comparações em tempo constante (chave do QR, token) | aqui |
| Formato da URI do QR (gerar e interpretar) | aqui |
| Prazos (handshake em 15 s, confirmação em 120 s) | aqui como **regra** (`deadline(now)`); o relógio é do consumidor |
| WebSocket, TLS, reconexão | consumidor |
| Gerar e guardar chaves (cofre do SO, Keychain/Keystore) | consumidor |
| Ler o motor VT e montar a grade; executar ações | Porecatu (`porecatu-remote` + `porecatu-ui`) |
| Desenhar a grade, teclado, telas | Remote |
| Contar canais, aplicar limites, rotear | Relay |

Isso mantém o crate encaixado nas regras de dependência do Porecatu: quem traz `tokio` é o crate de rede do Porecatu (`porecatu-remote`), não este.

## 3. Organização do código (P1 e P2)

```
rust/
  Cargo.toml                 crate porecatu-protocol, features: envelope (default-off p/ o relay), session, message
  rust-toolchain.toml
  src/
    envelope.rs              camada 2
    session/                 camada 3: handshake.rs, record.rs, rekey.rs
    message/                 camada 4: um módulo por grupo do §10 de 03
    pairing_uri.rs           porecatu://pair
    fingerprint.rs
  tests/
    vectors.rs               lê ../docs/especificacao/vetores-de-teste/**

ts/
  package.json               @porecatu/protocol
  src/
    envelope.ts
    session/                 handshake.ts, record.ts (Noise sobre @noble)
    message/
    pairingUri.ts
    fingerprint.ts
  test/
    vectors.test.ts          lê os mesmos JSON

schema/
  porecatu-remote.cddl       schema consolidado; o CI confere os vetores contra ele
```

## 4. Quem consome o quê

```
porecatu (desktop)
  └── porecatu-remote (crate folha, tokio + WebSocket)  ──►  porecatu-protocol [session, message, envelope]
porecatu-relay
  └── binário (tokio + axum)                           ──►  porecatu-protocol [envelope]
porecatu-remote (app Expo)
  └── camada de conexão (WebSocket do React Native)    ──►  @porecatu/protocol
```

Do lado do Porecatu, o desenho do crate `porecatu-remote` e do espelhamento da grade está nos ADRs 0103 a 0108 do Porecatu ([docs/adr](https://github.com/porecatu/terminal/tree/main/docs/adr)); do lado do app, na [arquitetura do Remote](https://github.com/porecatu/remote/blob/main/docs/arquitetura.md); do relay, na [arquitetura do Relay](https://github.com/porecatu/relay/blob/main/docs/arquitetura.md).

## 5. Estratégia de teste

1. **Vetores do Noise** (`cacophony`) para as duas implementações da camada 3, antes de qualquer vetor nosso.
2. **Vetores do Porecatu** ([formato](especificacao/vetores-de-teste/README.md)): ida e volta byte a byte, casos inválidos com o `error-code` esperado.
3. **Cruzado**: o CI gera sessões com o Rust e as reproduz no TS, e o contrário.
4. **Fuzz** dos decodificadores (envelope e mensagem) no lado Rust, com teto de tamanho e profundidade.
