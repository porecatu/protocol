# Arquitetura

Como o protocolo se organiza em camadas e como as implementações de referência ([ADR-0005](adr/0005-implementacoes-de-referencia.md)) se encaixam nos três projetos que o falam. A especificação diz **o quê**; este documento diz **onde cada coisa mora**.

**Estado:** especificação 0.3 (rascunho, versão de fio `[0, 1]`); o crate Rust e o pacote TypeScript implementam as camadas 2, 3 e 4 (P1 e P2 do [roadmap](roadmap.md)) e ainda não foram publicados. Este documento descreve o código como ele está.

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
| 2 — Envelope do relay | [01](especificacao/01-transporte-e-relay.md) §2–§5 | módulo `envelope`, sempre compilado (o Relay usa `default-features = false`, sem `session` e `message`) | módulo `envelope` (os dois lados, para o teste cruzado e o desktop falso do app — [ADR-0008](adr/0008-especificacao-0-2.md)) |
| 3 — Noise e registros | [02](especificacao/02-handshake-e-pareamento.md) | módulo `session`, feature `session` (sobre `snow` 0.10.0 sem a feature `std`) | módulo `session` (Noise próprio sobre `@noble/curves`, `@noble/ciphers` e `@noble/hashes`) |
| 4 — Mensagens | [03](especificacao/03-mensagens.md) | módulo `message`, feature `message`, sobre o CBOR escrito à mão de `cbor.rs` | módulo `message`, sobre o CBOR escrito à mão de `cbor/` |

O CBOR determinístico e estrito ([03](especificacao/03-mensagens.md) §1.1) é **escrito à mão nos dois lados**, codificador e decodificador; nem `ciborium` nem `cbor-x` participam ([ADR-0003](adr/0003-cbor-com-schema-em-cddl.md) e [ADR-0008](adr/0008-especificacao-0-2.md), notas de implementação). O `snow` fica sem `std` porque essa feature, na 0.10.0, liga as dependências opcionais `ring` e `sha2`.

## 2. O que as implementações de referência são, e o que não são

São **bibliotecas puras**: recebem bytes e o instante atual, devolvem bytes e eventos. Não abrem socket, não criam thread, não leem relógio, não guardam chave em disco.

| Responsabilidade | Onde fica |
|---|---|
| Codificar e decodificar envelope e mensagens | aqui |
| Máquina de estado do handshake (`XX`, `IK`), registros, fragmentação, rekey | aqui |
| Comparações em tempo constante (chave do QR, token de pareamento; o token do operador do relay é comparado pelo próprio relay) | aqui |
| Formato da URI do QR (gerar e interpretar) | aqui |
| Prazos do handshake (15 s, `HANDSHAKE_TIMEOUT_MS`) e validade do token de pareamento (5 min, `TOKEN_TTL_MS`); prazos do envelope (`REGISTER_TIMEOUT_MS`, `PONG_TIMEOUT_MS`) como constantes | aqui como **regra** sobre um `now` recebido (`tick(now)` e `next_deadline()` nas máquinas de estado); o relógio é do consumidor |
| Prazo de 120 s do diálogo de confirmação do pareamento ([02](especificacao/02-handshake-e-pareamento.md) §3.2) | consumidor (o host): as bibliotecas não têm constante nem regra para ele |
| WebSocket, TLS, reconexão | consumidor |
| Gerar e guardar chaves (cofre do SO, Keychain/Keystore) | consumidor |
| Ler o motor VT e montar a grade; executar ações | Porecatu (`porecatu-remote` + `porecatu-ui`) |
| Desenhar a grade, teclado, telas | Remote |
| Contar canais, aplicar limites, rotear | Relay |

Isso mantém o crate encaixado nas regras de dependência do Porecatu: quem traz `tokio` é o crate de rede do Porecatu (`porecatu-remote`), não este.

## 3. Organização do código

```
rust-toolchain.toml          toolchain Rust pinada (1.98.0), na raiz do repositório

rust/
  Cargo.toml                 workspace + crate porecatu-protocol; features: default = [session, message],
                             test-vectors (efêmeros e nonces fixos, só em teste); o envelope é sempre compilado
  deny.toml                  cargo deny: licenças, fontes, RUSTSEC
  src/
    lib.rs                   PROTOCOL_VERSION = [0, 1] (versão de fio) e reexportações
    cbor.rs                  CBOR determinístico e estrito, escrito à mão (03 §1.1)
    envelope.rs              camada 2, os dois lados
    session/                 camada 3: keys, handshake, record, rekey, pairing_token,
                             client (ClientSession), host (HostChannel)
    message/                 camada 4: um módulo por grupo do §10 de 03
    host_id.rs, fingerprint.rs, pairing_uri.rs, ct.rs, encoding.rs
  tests/
    vectors.rs               reproduz todos os vetores, inclusive sessoes-ts/
    full_session.rs          host e cliente em memória: pareamento, conexão, prazos, recusas
  tools/vectors/             ferramenta não publicada: generate, check, schema

ts/
  package.json               @porecatu/protocol; exports "." e "./testing"
  pnpm-workspace.yaml        configuração do pnpm (allowBuilds etc.)
  src/
    cbor/                    CBOR determinístico e estrito, escrito à mão
    envelope.ts              camada 2, os dois lados
    session/                 camada 3: Noise próprio sobre @noble (handshakeState, symmetricState,
                             cipherState), record, rekey, pairingToken, client, host
    message/                 camada 4
    hostId.ts, fingerprint.ts, pairingUri.ts, ct.ts, encoding.ts
    testing/                 @porecatu/protocol/testing: relay em memória, desktop falso, chaves e relógio de teste
  test/
    vectors*.test.ts         reproduzem os mesmos JSON
    transcripts.test.ts      grava e confere sessoes-ts/

schema/
  porecatu-remote.cddl       schema consolidado, gerado por scripts/extract-cddl.py; `vectors check` confere os vetores contra ele
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

1. **Vetores do Noise** (`cacophony`, como o `snow` os distribui) para as duas implementações da camada 3, antes de qualquer vetor nosso.
2. **Vetores do Porecatu** ([formato](especificacao/vetores-de-teste/README.md)): ida e volta byte a byte, casos inválidos com o código esperado; os de valor também contra o CDDL consolidado (`vectors check`).
3. **Cruzado**: as sessões geradas pelo Rust (`sessoes/`) são reproduzidas pelo TS, e as gravadas pelo TS (`sessoes-ts/`) são reproduzidas pelo Rust, nas camadas de handshake e registros e com as máquinas de estado. Os arquivos ficam commitados; o CI roda as duas suítes sobre eles.
4. **Integração em memória**: host e cliente das duas implementações conversam sem rede (`rust/tests/full_session.rs`, `ts/test/fullSession.test.ts`), com prazos, recusas e mensagens grandes.
5. **Fuzz** dos decodificadores (envelope e mensagem) no lado Rust, com teto de tamanho e profundidade: **ainda não feito**.
