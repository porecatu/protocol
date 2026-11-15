# CLAUDE.md

Guia operacional do repositório. Leia antes de mexer em qualquer coisa.

## O que é

**Porecatu Protocol** — a especificação normativa do acesso remoto do Porecatu, mais (a partir da P1) as duas implementações de referência e os vetores de teste. Três projetos dependem dele:

| Projeto | Papel | Consome |
|---|---|---|
| [porecatu/terminal](https://github.com/porecatu/terminal) | host (desktop) | crate Rust `porecatu-protocol` |
| [porecatu/relay](https://github.com/porecatu/relay) | relay cego | crate Rust (só o envelope, camada 2) |
| [porecatu/remote](https://github.com/porecatu/remote) | cliente (Expo) | pacote TS `@porecatu/protocol` |

**Estado:** especificação **0.1, rascunho** (P0 do [roadmap](docs/roadmap.md)). Nenhum código ainda. A versão de fio `[0, 1]` congela quando os três projetos publicarem juntos a primeira versão ([04 — Versionamento](docs/especificacao/04-versionamento.md)).

## A especificação é normativa

- O texto em [docs/especificacao/](docs/especificacao/00-visao-geral.md) manda. Divergência entre texto e implementação é bug da implementação ou, se o texto estava errado, correção do texto **aqui primeiro**.
- Os **vetores de teste** são o árbitro entre as duas implementações ([ADR-0005](docs/adr/0005-implementacoes-de-referencia.md)). Mudança no fio sem vetor novo não entra.
- Palavras DEVE / NÃO DEVE / DEVERIA / PODE têm o peso da RFC 2119.
- **Nenhuma prosa no fio**: erro e motivo são códigos; quem mostra compõe o texto.

### Mudar a especificação

O fluxo está em [04 — Versionamento, "Mudar esta especificação"](docs/especificacao/04-versionamento.md#mudar-esta-especificação). Em resumo: decisão nova → ADR; texto e versão da especificação no mesmo PR; mexeu no fio → `[major, minor]` sobe, vetor novo, [CHANGELOG](CHANGELOG.md); implementações acompanham em PRs próprios.

Enquanto `major = 0`, qualquer mudança pode quebrar e os lados exigem `minor` igual.

## Implementações de referência (a partir da P1)

Layout previsto:

```
rust/            crate `porecatu-protocol`
ts/              pacote `@porecatu/protocol`
schema/          porecatu-remote.cddl (fonte única do schema, gerada dos blocos da especificação)
docs/especificacao/vetores-de-teste/   JSON, lidos pelos testes das duas implementações
```

- **Rust** ([ADR-0007](docs/adr/0007-toolchains.md)): stable **pinada** em `rust-toolchain.toml`, edition 2024, lints no `[lints]`, `unsafe_code = "deny"` — mesma disciplina do ADR-0011 do Porecatu. `snow` (Noise) e `ciborium` (CBOR), **versões exatas** (`=x.y.z`); subir é tarefa própria. O crate é **síncrono e sem runtime**: não traz `tokio`, nem WebSocket. Transporte é problema de quem consome.
- **TypeScript**: `strict`, sem `any` implícito; `@noble/curves`, `@noble/ciphers`, `@noble/hashes` (JS puro, roda no Expo sem módulo nativo) e `cbor-x`, versões exatas no `package.json`, lockfile versionado, `pnpm` pinado.
- As duas leem os **mesmos** vetores, e o CI roda as duas.

## Comandos (quando houver código)

```bash
python scripts/verify-docs.py          # já existe: links relativos da documentação

cargo test  --manifest-path rust/Cargo.toml
cargo clippy --manifest-path rust/Cargo.toml --all-targets -- -D warnings
cargo fmt   --manifest-path rust/Cargo.toml --check

pnpm -C ts install --frozen-lockfile
pnpm -C ts test
pnpm -C ts lint
```

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

Correção de erro factual ou de clareza é permitida; mudança de decisão, não. Revisão parcial entra como blockquote na seção revista, apontando o ADR novo.

## Índice

- [docs/especificacao/](docs/especificacao/00-visao-geral.md) — a especificação
- [docs/prd/prd-000-visao-de-produto.md](docs/prd/prd-000-visao-de-produto.md) — por que o protocolo existe e o que fica fora do v0
- [docs/arquitetura.md](docs/arquitetura.md) — camadas e implementações
- [docs/adr/](docs/adr/0000-template.md) — decisões
- [docs/roadmap.md](docs/roadmap.md) — fases e critérios de saída
