# Roadmap

Fases do protocolo. Cada fase tem **critério de saída explícito**; enquanto ele não for atendido, a fase não terminou. As fases deste repositório andam junto com as dos consumidores: o Porecatu (PRD-025, acesso remoto), o [Relay](https://github.com/porecatu/relay/blob/main/docs/roadmap.md) e o [Remote](https://github.com/porecatu/remote/blob/main/docs/roadmap.md).

| Fase | Status |
|---|---|
| P0 — Especificação 0.1 (rascunho) | **em andamento** |
| P1 — Crate Rust e vetores | planejada |
| P2 — Pacote TypeScript | planejada |
| P3 — Congelar `[0, 1]` | planejada |

---

## P0 — Especificação 0.1 (rascunho)

Escrever o contrato antes do código, para os três projetos citarem a mesma coisa.

**Entregas:** [especificação](especificacao/00-visao-geral.md) (00 a 04, modelo de ameaça, formato dos vetores), [ADR-0001 a ADR-0007](adr/0000-template.md), [PRD-000](prd/prd-000-visao-de-produto.md), esta documentação.

**Critério de saída:**

1. Os documentos de especificação existem e o `scripts/verify-docs.py` passa.
2. O PRD-025 do Porecatu, a documentação inicial do Relay e a do Remote citam a especificação sem contradizê-la.
3. O dono do produto revisou o modelo de ameaça e as decisões em aberto da especificação.

## P1 — Crate Rust e vetores

**Entregas:** `rust/` com as camadas 2, 3 e 4 ([arquitetura](arquitetura.md) §3); `schema/porecatu-remote.cddl` consolidado; vetores de teste dos casos obrigatórios; CI (fmt, clippy, testes, `cargo deny`, verificação de docs).

**Critério de saída:**

1. O crate passa nos vetores do `cacophony` para `XX` e `IK` com o prólogo do Porecatu.
2. Todo tipo de mensagem do §10 de [03](especificacao/03-mensagens.md) tem os cinco casos obrigatórios de vetor.
3. Os vetores validam contra o CDDL consolidado.
4. O crate compila com a toolchain pinada do Porecatu e sem `tokio` na árvore (`cargo tree -i tokio` vazio).
5. Uma sessão completa (pareamento `XX` → `hello` → `workspace` → assinatura → diffs → ação → `goodbye`) roda em teste de integração, host e cliente em memória.

## P2 — Pacote TypeScript

**Entregas:** `ts/` com as mesmas camadas; CI com testes, lint e auditoria.

**Critério de saída:**

1. O pacote passa em **todos** os vetores da P1, sem exceção nem vetor marcado como "pular".
2. Teste cruzado: sessão gerada pelo Rust reproduzida pelo TS e vice-versa.
3. O pacote roda no Hermes do Expo (teste de fumaça no app do Remote), sem APIs do Node.

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
