# Roadmap

Fases do protocolo. Cada fase tem **critério de saída explícito**; enquanto ele não for atendido, a fase não terminou. As fases deste repositório andam junto com as dos consumidores: o Porecatu (PRD-025, acesso remoto), o [Relay](https://github.com/porecatu/relay/blob/main/docs/roadmap.md) e o [Remote](https://github.com/porecatu/remote/blob/main/docs/roadmap.md).

| Fase | Status |
|---|---|
| P0 — Especificação 0.1 (rascunho) | entregas feitas, hoje na **0.4**; critério 3 sem registro |
| P1 — Crate Rust e vetores | **entregue**, com dívida de verificação |
| P2 — Pacote TypeScript | **entregue**, com dívida de verificação (Hermes) |
| P3 — Congelar `[0, 1]` | planejada |

Medido em 2026-11-17, no Windows: `cargo test --workspace` (em `rust/`) com 269 testes passando e um doctest ignorado; `pnpm test` (em `ts/`) com 744 testes passando em 22 arquivos e um pulado (o que regrava `sessoes-ts/`, que só roda com `WRITE_TRANSCRIPTS`); `cargo run -p vectors -- check` com 334 vetores e nenhuma falha; `python scripts/verify-docs.py` passando.

---

## P0 — Especificação 0.1 (rascunho)

Escrever o contrato antes do código, para os três projetos citarem a mesma coisa.

**Entregas:** [especificação](especificacao/00-visao-geral.md) (00 a 04, modelo de ameaça, formato dos vetores), [ADR-0001 a ADR-0007](adr/0000-template.md), [PRD-000](prd/prd-000-visao-de-produto.md), esta documentação.

**Critério de saída:**

1. Os documentos de especificação existem e o `scripts/verify-docs.py` passa.
2. O PRD-025 do Porecatu, a documentação inicial do Relay e a do Remote citam a especificação sem contradizê-la.
3. O dono do produto revisou o modelo de ameaça e as decisões em aberto da especificação.

**Situação:** a especificação passou por 0.2 ([ADR-0008](adr/0008-especificacao-0-2.md)), 0.3 ([ADR-0009](adr/0009-grafema-no-fio-e-linhas-descartadas.md)) e 0.4 ([ADR-0010](adr/0010-aba-ativa-pelo-cliente.md)) e foi implementada nas duas linguagens. Critério 1 atendido. Critério 2 não foi conferido nesta leva (é leitura dos outros repositórios). Critério 3 não tem registro neste repositório.

## P1 — Crate Rust e vetores

**Entregas:** `rust/` com as camadas 2, 3 e 4 ([arquitetura](arquitetura.md) §3); `schema/porecatu-remote.cddl` consolidado; vetores de teste dos casos obrigatórios; CI (fmt, clippy, testes, `cargo deny`, verificação de docs).

**Critério de saída:**

1. O crate passa nos vetores do `cacophony` para `XX` e `IK` com o prólogo do Porecatu.
2. Todo tipo de mensagem do §10 de [03](especificacao/03-mensagens.md) tem os cinco casos obrigatórios de vetor.
3. Os vetores validam contra o CDDL consolidado.
4. O crate compila com a toolchain pinada do Porecatu e sem `tokio` na árvore (`cargo tree -i tokio` vazio).
5. Uma sessão completa (pareamento `XX` → `hello` → `workspace` → assinatura → diffs → ação → `goodbye`) roda em teste de integração, host e cliente em memória.

**Situação: entregue.** `rust/` (crate e ferramenta `rust/tools/vectors`), `schema/porecatu-remote.cddl`, 334 vetores e o CI (`.github/workflows/ci.yml`) existem.

1. Atendido: `rust/tests/vectors.rs` reproduz os quatro vetores do cacophony (como o `snow` 0.10.0 os distribui) e os cinco de `noise/` com o prólogo do Porecatu.
2. Atendido: as 26 mensagens e o `pair-request` têm mínimo, completo, chave desconhecida, uma recusa por chave obrigatória ausente e não canônico; as de grade, os casos extras.
3. Atendido: `vectors check` valida os vetores de valor contra o CDDL consolidado.
4. Atendido: toolchain 1.98.0, a mesma do Porecatu; `cargo tree -p porecatu-protocol -e normal -i tokio` não acha nada (o CI confere).
5. Atendido **em partes**, não numa conversa só: `rust/tests/full_session.rs` faz pareamento `XX` → `pair-accepted` → `hello`/`welcome` → `subscribe` → `grid-snapshot` → `grid-ack` com host e cliente em memória, e `goodbye` nos dois sentidos noutro teste; `workspace` vem das sessões de `sessoes/`, e `grid-diff` e ação com `action-result` vêm das transcrições de `sessoes-ts/`, que o crate reproduz com `ClientSession` e `HostChannel`.

**Dívida de verificação:** Linux e macOS só no CI (a verificação local foi no Windows); `cargo deny` só no CI, não rodado localmente; fuzz dos decodificadores ([arquitetura](arquitetura.md) §5) não feito.

## P2 — Pacote TypeScript

**Entregas:** `ts/` com as mesmas camadas; CI com testes, lint e auditoria.

**Critério de saída:**

1. O pacote passa em **todos** os vetores da P1, sem exceção nem vetor marcado como "pular".
2. Teste cruzado: sessão gerada pelo Rust reproduzida pelo TS e vice-versa.
3. O pacote roda no Hermes do Expo (teste de fumaça no app do Remote), sem APIs do Node.

**Situação: entregue, com o critério 3 pendente.** `ts/` com as camadas 2, 3 e 4 nos dois papéis e a entrada `@porecatu/protocol/testing`; CI com typecheck, lint, testes e build.

1. Atendido: `pnpm test` reproduz todos os vetores (identidade, envelope, mensagens, Noise com o cacophony, registros, sessões); o único teste pulado é o que regrava `sessoes-ts/`, e nenhum vetor é pulado.
2. Atendido **nos dois sentidos**: o TS reproduz as sessões que o Rust gera (`sessoes/`) e grava as suas (`sessoes-ts/`: pareamento aceito, `IK` pareado, `IK` desconhecido), que o Rust reproduz byte a byte nas camadas de handshake e registros e com as máquinas de estado. Nenhuma divergência.
3. **Pendente:** `src/` não usa APIs do Node (sem `Buffer`, `node:*`, `process`, `TextDecoder` obrigatório nem `Intl.Segmenter`), mas a execução no Hermes não foi vista: depende do app do Remote e de aparelho ou emulador, que não há nesta máquina.

**Dívida de verificação:** Hermes (critério 3); auditoria de dependências e de licenças do `pnpm` no CI, prevista nas entregas e no [ADR-0007](adr/0007-toolchains.md), ainda não existe; o pacote só roda no Linux do CI.

## P3 — Congelar `[0, 1]`

**Entregas:** versão de fio `[0, 1]` publicada junto com a primeira versão do acesso remoto no Porecatu, do Relay e do Remote; crate e pacote publicados com a mesma tag.

**Critério de saída:**

1. Os três consumidores dependem da mesma versão do crate/pacote e conversam ponta a ponta através de um relay real.
2. O [CHANGELOG](../CHANGELOG.md) registra a versão.
3. Qualquer mudança no fio daí em diante segue [04 — Versionamento](especificacao/04-versionamento.md).

---

## Fora do v0

Registrado para não ser reinventado como ideia nova. Justificativa no [PRD-000](prd/prd-000-visao-de-produto.md).

| Item | Observação |
|---|---|
| Notificações push | Exige APNs/FCM no relay e payload cego |
| Conexão direta na rede local | Mesma sessão Noise, outro transporte |
| Cliente web | Servir o app pelo relay contradiz o relay cego |
| Pareamento por código digitado | Exige PAKE (ex.: SPAKE2) |
| Permissões por aparelho (somente leitura) | O v0 dá acesso total |
| Modo em segundo plano do host | É do ciclo de vida do Porecatu, não do protocolo |
| QUIC como transporte | Revisitar quando o suporte no React Native amadurecer |
| `major = 1` | Depois de uso real com `[0, 1]` |
