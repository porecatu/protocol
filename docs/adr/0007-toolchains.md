# ADR-0007 — Toolchains pinadas: Rust stable e Node LTS com `pnpm`

**Status:** Aceito
**Data:** 2026-11-15
**Relacionados:** [ADR-0005](0005-implementacoes-de-referencia.md); ADR-0011 do Porecatu ([toolchain Rust](https://github.com/porecatu/terminal/blob/main/docs/adr/0011-toolchain-rust.md))

## Contexto

Este repositório terá código em duas linguagens ([ADR-0005](0005-implementacoes-de-referencia.md)), consumido pelo Porecatu (Rust, toolchain pinada pelo ADR-0011 dele), pelo Relay (Rust) e pelo Remote (TypeScript no Expo). Um vetor de teste que passa numa máquina e falha no CI por diferença de versão de compilador ou de biblioteca é exatamente o tipo de ruído que o árbitro não pode ter.

## Decisão

**As duas toolchains são pinadas e as dependências são fixadas em versão exata; subir qualquer uma é tarefa própria, com PR próprio.**

### Rust (`rust/`)

- Canal **stable pinado** em `rust/rust-toolchain.toml`, a **mesma versão** que o Porecatu usa no momento da criação do crate; edition 2024; `rust-version` no manifesto.
- Lints no `[lints]` do crate, no padrão do Porecatu: `unsafe_code = "deny"`, clippy com `-D warnings` no CI.
- Dependências em **versão exata** (`snow = "=…"`, `ciborium = "=…"`), `Cargo.lock` versionado, `cargo deny` (licenças e RUSTSEC) no CI.
- O crate **não pode** exigir toolchain mais nova que a do Porecatu: quem sobe primeiro é o Porecatu.

> **Nota de implementação (2026-11-16):** o `rust-toolchain.toml` ficou na **raiz** do repositório, não em `rust/`, com `channel = "1.98.0"` (a mesma do Porecatu); `rust/Cargo.toml` é um workspace com o crate e a ferramenta de vetores `rust/tools/vectors`. O `snow =0.10.0` entra **sem a feature `std`**: nessa versão, `std` ativa `ring/std` e `sha2/std`, o que liga as dependências opcionais `ring` e `sha2`; sem ela, nenhuma das duas entra na árvore. O `cargo deny` roda no CI (`rust/deny.toml`), não localmente.

### TypeScript (`ts/`)

- **Node LTS** pinado em `.nvmrc` e em `engines`; **`pnpm`** pinado pelo campo `packageManager` (Corepack); `pnpm-lock.yaml` versionado, `--frozen-lockfile` no CI.
- `tsconfig` com `strict: true`, `noUncheckedIndexedAccess`, `exactOptionalPropertyTypes`; saída ESM; alvo compatível com o Hermes do Expo.
- Dependências em versão exata (`"@noble/curves": "x.y.z"`, sem `^`), auditoria de dependências e de licenças no CI.
- Testes com um runner que roda em Node (ex.: `vitest`), lendo os mesmos vetores JSON.

> **Nota de implementação (2026-11-16):** Node 26.13.0 em `ts/.nvmrc` e `engines` (`>=26.13.0`), `pnpm@12.17.0` em `packageManager`, `vitest` 5.1.0, TypeScript 6.0.3. A configuração do `pnpm` (`allowBuilds`, `nodeLinker` e afins) mora em **`ts/pnpm-workspace.yaml`**: o pnpm 12 não a lê do `.npmrc`, e o pacote não tem `.npmrc`. O CI do pacote roda typecheck, lint, testes e build; a **auditoria de dependências e de licenças do `pnpm` ainda não está no CI** (dívida, no [roadmap](../roadmap.md)).

## Alternativas consideradas

- **Rust `stable` flutuante.** Menos manutenção. Rejeitada pelo mesmo motivo do ADR-0011 do Porecatu: lint ou comportamento novo quebra o CI num dia qualquer, sem PR que explique.
- **`npm` ou `yarn`.** Funcionam. `pnpm` foi preferido pelo lockfile estrito e pela resolução sem dependências fantasmas; o Remote (Expo) usa o mesmo gerenciador, o que simplifica testar o pacote localmente no app.
- **Bun ou Deno.** Rápidos. Rejeitados: o alvo de execução real é o Hermes do Expo, e Node é o ambiente que o ecossistema Expo testa.
- **Faixas semver (`^`) nas dependências TS.** Atualizações automáticas. Rejeitada: criptografia e codificação são exatamente onde uma subida silenciosa não pode acontecer.

## Consequências

### Positivas

- Build reproduzível; falha de vetor é sempre falha de código.
- Mesma disciplina do Porecatu, conhecida por quem já contribui lá.

### Negativas

- Subidas de toolchain e de dependências são trabalho manual recorrente (o Dependabot ajuda a abrir os PRs).
- O crate fica preso à toolchain do Porecatu.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Vulnerabilidade numa dependência fixada | Média | Alto | `cargo deny`/RUSTSEC e auditoria do `pnpm` no CI; subida de segurança tem prioridade |
| Hermes não suportar algo que o Node suporta | Média | Médio | Sem APIs de Node no pacote (sem `Buffer`, `crypto` do Node); teste de fumaça no app Expo do Remote |
