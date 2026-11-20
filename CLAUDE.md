# CLAUDE.md

Guia operacional do repositório. Leia antes de mexer em qualquer coisa.

## O que é

**Porecatu Protocol** — a especificação normativa do acesso remoto do Porecatu, as duas implementações de referência e os vetores de teste. Três projetos dependem dele:

| Projeto | Papel | Consome |
|---|---|---|
| [porecatu/terminal](https://github.com/porecatu/terminal) | host (desktop) | crate Rust `porecatu-protocol` |
| [porecatu/relay](https://github.com/porecatu/relay) | relay cego | crate Rust (só o envelope, camada 2) |
| [porecatu/remote](https://github.com/porecatu/remote) | cliente (Expo) | pacote TS `@porecatu/protocol` |

**Estado:** especificação **0.5, rascunho** ([ADR-0011](docs/adr/0011-cliente-nao-muda-a-aba-ativa-e-view-tab.md): `view-tab` entra, `activate-tab` sai); crate Rust e pacote TypeScript implementados (P1 e P2 do [roadmap](docs/roadmap.md)), nada publicado. A versão de fio `[0, 1]` congela quando os três projetos publicarem juntos a primeira versão ([04 — Versionamento](docs/especificacao/04-versionamento.md)).

- **Duas versões, não confundir:** `PROTOCOL_VERSION = [0, 1]` (no crate e no pacote) é a **versão de fio**, a que vai no prólogo (só o `major`), no QR e no `hello`; **0.5** é a versão do **texto** da especificação, no cabeçalho de cada documento. A 0.2, a 0.3, a 0.4 e a 0.5 mudaram o fio sem subir `[0, 1]`, porque ele ainda não congelou ([ADR-0008](docs/adr/0008-especificacao-0-2.md), [ADR-0009](docs/adr/0009-grafema-no-fio-e-linhas-descartadas.md), [ADR-0010](docs/adr/0010-aba-ativa-pelo-cliente.md), [ADR-0011](docs/adr/0011-cliente-nao-muda-a-aba-ativa-e-view-tab.md)). As versões semânticas do crate e do pacote (`0.1.0`) são uma terceira coisa.
- **Verificado** (Windows, 2026-11-20): `cargo test --workspace` com 273 testes, `pnpm test` com 755 (um pulado: o que regrava `sessoes-ts/`), `vectors check` com 340 vetores, teste cruzado Rust↔TS nos dois sentidos sem divergência.
- **Dívida de verificação:** Linux e macOS (o CI roda o crate nas três plataformas e o pacote no Linux), `cargo deny` (só no CI), o pacote no Hermes do Expo (teste de fumaça no app do Remote), publicação. Sem fuzz dos decodificadores e sem auditoria de dependências do `pnpm` no CI ([roadmap](docs/roadmap.md)).

## A especificação é normativa

- O texto em [docs/especificacao/](docs/especificacao/00-visao-geral.md) manda. Divergência entre texto e implementação é bug da implementação ou, se o texto estava errado, correção do texto **aqui primeiro**.
- Os **vetores de teste** são o árbitro entre as duas implementações ([ADR-0005](docs/adr/0005-implementacoes-de-referencia.md)). Mudança no fio sem vetor novo não entra.
- Palavras DEVE / NÃO DEVE / DEVERIA / PODE têm o peso da RFC 2119.
- **Nenhuma prosa no fio**: erro e motivo são códigos; quem mostra compõe o texto.

### Mudar a especificação

O fluxo está em [04 — Versionamento, "Mudar esta especificação"](docs/especificacao/04-versionamento.md#mudar-esta-especificação). Em resumo: decisão nova → ADR; texto e versão da especificação no mesmo PR; mexeu no fio → `[major, minor]` sobe, vetor novo, [CHANGELOG](CHANGELOG.md); implementações acompanham em PRs próprios.

Enquanto `major = 0`, qualquer mudança pode quebrar e os lados exigem `minor` igual.

## Implementações de referência

```
rust-toolchain.toml   toolchain Rust pinada (1.98.0), na raiz do repositório
rust/                 workspace: crate `porecatu-protocol` (raiz) + `tools/vectors` (não publicada)
ts/                   pacote `@porecatu/protocol` (+ entrada `@porecatu/protocol/testing`)
schema/               porecatu-remote.cddl, gerado dos blocos `cddl` da especificação (scripts/extract-cddl.py)
docs/especificacao/vetores-de-teste/   JSON, lidos pelos testes das duas implementações
```

- **Bibliotecas sans-IO**: bytes e `now` entram, bytes e eventos saem. Sem socket, thread, relógio, disco, `tokio` ou WebSocket nas duas; prazo é regra sobre um `now` recebido (`tick`/`next_deadline`). Erro é código tipado, nunca prosa.
- **Rust** ([ADR-0007](docs/adr/0007-toolchains.md)): stable **pinada** em `rust-toolchain.toml` (raiz), edition 2024, `rust-version = "1.98.0"`, lints no `[lints]`, `unsafe_code = "deny"` — mesma disciplina do ADR-0011 do Porecatu. Dependências em **versão exata** (`=x.y.z`), `Cargo.lock` versionado; subir é tarefa própria.
  - Features: `default = ["session", "message"]`; o **envelope é sempre compilado**, e o Relay usa `default-features = false`. A feature `test-vectors` injeta chaves efêmeras e nonces fixos, só para testes e para a ferramenta de vetores; nunca em produção.
  - `snow =0.10.0` **sem a feature `std`**: no `snow` 0.10.0, `std` ativa `ring/std` e `sha2/std`, o que liga as dependências opcionais `ring` e `sha2`; sem ela, nenhuma das duas entra na árvore (o `snow` compila em `no_std` + `alloc`, com o resolvedor padrão e `curve25519`, `chacha20poly1305`, `blake2` e `getrandom`).
  - **CBOR escrito à mão** (`rust/src/cbor.rs`): codificador determinístico e decodificador estrito de [03](docs/especificacao/03-mensagens.md) §1.1. O crate não depende do `ciborium`; ele só aparece na árvore da ferramenta `vectors`, trazido pelo `cddl`.
  - Sem `tokio` na árvore normal: `cargo tree -p porecatu-protocol -e normal -i tokio` não acha nada (o CI confere).
- **TypeScript** ([ADR-0007](docs/adr/0007-toolchains.md)): ESM (`"type": "module"`, mapa `exports`), saída `dist/` com `.d.ts`; `strict`, `noUncheckedIndexedAccess`, `exactOptionalPropertyTypes`; **nenhuma API do Node em `src/`** (sem `Buffer`, `node:*`, `process`). Dependências de execução: só `@noble/curves`, `@noble/ciphers` e `@noble/hashes` 2.4.0 (JS puro, roda no Expo sem módulo nativo), versões exatas, `pnpm-lock.yaml` versionado, `pnpm` 12.17.0 pinado em `packageManager`, Node 26.13.0 em `ts/.nvmrc`.
  - **CBOR escrito à mão** (`ts/src/cbor/`), codificador e decodificador; o `cbor-x` foi removido.
  - Configuração do `pnpm` (`allowBuilds`, `nodeLinker` e afins) mora em `ts/pnpm-workspace.yaml`: o pnpm 12 não a lê do `.npmrc`.
- As duas leem os **mesmos** vetores; o Rust reproduz as sessões que o TS grava (`sessoes-ts/`) e o TS reproduz tudo o que o Rust gera ([vetores](docs/especificacao/vetores-de-teste/README.md)). O CI roda as duas.

## Comandos

```bash
python scripts/verify-docs.py          # links relativos e schema CDDL em sincronia com a especificação
python scripts/extract-cddl.py         # regera schema/porecatu-remote.cddl

# dentro de rust/
cargo build --workspace
cargo test  --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt   --all --check
cargo run -p vectors -- generate       # regera os vetores (menos sessoes-ts/ e o cacophony)
cargo run -p vectors -- check          # confere os vetores contra o crate e o CDDL
cargo run -p vectors -- schema         # confere que o CDDL consolidado é válido

# dentro de ts/
pnpm install --frozen-lockfile
pnpm test
pnpm lint
pnpm typecheck
pnpm build
WRITE_TRANSCRIPTS=1 pnpm exec vitest run test/transcripts.test.ts   # regrava sessoes-ts/; depois, cargo test em rust/
```

**Armadilha:** rode os testes Rust **sempre com `--workspace`**. Os vetores de Noise com o prólogo do Porecatu, os de registros e os de sessão precisam da feature `test-vectors` (efêmeros fixos), que só liga porque a ferramenta `vectors` do mesmo workspace depende dela; um `cargo test -p porecatu-protocol` sozinho compila esses casos como vazios e passa sem reproduzir nada.

## Convenções

- **Código, identificadores, comentários de código e nomes de arquivo de código: inglês.**
- **Documentação (README, ADR, PRD, especificação, docs/) e mensagens de commit: português do Brasil.**
- Commits em [Conventional Commits](https://www.conventionalcommits.org) (`feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`, `ci:`), com **sign-off do DCO** (`git commit -s`, [CONTRIBUTING](CONTRIBUTING.md)).
- Arquivos de doc em `kebab-case.md`.
- Links para os outros repositórios são **URLs absolutas** (`https://github.com/porecatu/...`); o `verify-docs.py` reprova link relativo que não resolve.
- Actions do GitHub pinadas por SHA de commit, com a versão no comentário, como no Porecatu.

## Licença

**GPL-3.0-or-later** ([ADR-0006](docs/adr/0006-licenca-gpl.md)). Todo arquivo de código-fonte novo começa com:

```rust
// SPDX-License-Identifier: GPL-3.0-or-later
```

Sem exceção — inclusive scripts (`# SPDX-License-Identifier: GPL-3.0-or-later` em Python). `LICENSE` é cópia verbatim da FSF (a mesma do Porecatu, sha256 `3972dc97…6986`) e **nunca é editado**. Antes de adotar dependência nova, conferir se a licença é compatível com GPLv3.

## Processo de decisão

Igual ao do Porecatu. Decisão aceita **não se edita**:

1. Escreva um ADR novo com o próximo número, com `Supersedes: ADR-NNNN`.
2. Marque o antigo como `Status: Superseded by ADR-MMMM`.
3. Atualize o índice em [docs/adr/0000-template.md](docs/adr/0000-template.md).

Correção de erro factual ou de clareza é permitida; mudança de decisão, não. Revisão parcial entra como blockquote na seção revista, apontando o ADR novo. Onde o código seguiu outro caminho sem mudar a decisão (biblioteca, caminho de arquivo), a seção ganha uma **nota de implementação** datada, também em blockquote.

## Índice

- [docs/especificacao/](docs/especificacao/00-visao-geral.md) — a especificação
- [docs/prd/prd-000-visao-de-produto.md](docs/prd/prd-000-visao-de-produto.md) — por que o protocolo existe e o que fica fora do v0
- [docs/arquitetura.md](docs/arquitetura.md) — camadas e implementações
- [docs/adr/](docs/adr/0000-template.md) — decisões
- [docs/roadmap.md](docs/roadmap.md) — fases e critérios de saída
- [docs/especificacao/vetores-de-teste/README.md](docs/especificacao/vetores-de-teste/README.md) — formato dos vetores, quem gera, quem reproduz, como regenerar
- [CHANGELOG.md](CHANGELOG.md) — mudanças da especificação e das implementações
