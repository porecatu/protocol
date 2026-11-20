# Porecatu Protocol

Especificação do protocolo de **acesso remoto do Porecatu**: o contrato que o Porecatu desktop, o Porecatu Relay e o Porecatu Remote seguem para que um celular alcance uma sessão viva do terminal sem que o servidor do meio consiga ler o conteúdo.

**Status:** especificação **0.6, em rascunho** (o que a 0.6 acrescenta, sessões salvas pelo cliente, ainda não está implementado), versão de fio **`[0, 1]`**, ainda não congelada (são duas versões diferentes: a do texto e a do que vai no fio — [04 — Versionamento](docs/especificacao/04-versionamento.md)). As duas implementações de referência e os vetores de teste existem e concordam entre si; nada foi publicado ainda ([roadmap](docs/roadmap.md)).

## Implementações de referência

| | Onde | O que tem |
|---|---|---|
| Crate Rust `porecatu-protocol` | [`rust/`](rust/Cargo.toml) | envelope do relay (sempre compilado), Noise `XX`/`IK` e registros (feature `session`), as 27 mensagens CBOR (feature `message`), `host_id`, impressão digital, URI de pareamento; sans-IO, sem `tokio` |
| Pacote TypeScript `@porecatu/protocol` | [`ts/`](ts/package.json) | as mesmas camadas, nos dois papéis (cliente e host); entrada `@porecatu/protocol/testing` com relay e desktop falsos em memória; sem APIs do Node |
| Ferramenta de vetores | [`rust/tools/vectors`](rust/tools/vectors/Cargo.toml) | `generate`, `check` e `schema`; não publicada |
| Vetores de teste | [`docs/especificacao/vetores-de-teste/`](docs/especificacao/vetores-de-teste/README.md) | 340 vetores, lidos pelas duas implementações; o Rust reproduz as sessões gravadas pelo TS e vice-versa |
| Schema CDDL | [`schema/porecatu-remote.cddl`](schema/porecatu-remote.cddl) | gerado dos blocos `cddl` da especificação por `scripts/extract-cddl.py` |

Verificado nesta máquina (Windows) em 2026-11-20: `cargo test --workspace` com 273 testes passando (e um doctest ignorado), `pnpm test` com 755 testes passando (e o que regrava `sessoes-ts/`, pulado de propósito), `vectors check` com 334 vetores e nenhuma falha. **Dívida de verificação:** Linux e macOS, `cargo deny` e a execução do pacote no Hermes do Expo só rodam no CI ou no app do Remote, não foram vistos aqui.

## As três partes

```
Porecatu desktop (host) <--wss--> Porecatu Relay (cego) <--wss--> Porecatu Remote (app)
          ^                                                                ^
          +------------------- sessão Noise ponta a ponta -----------------+
```

| Parte | O que faz | Repositório |
|---|---|---|
| **Host** | O Porecatu desktop. Dono do estado: workspace, PTYs, motor VT, tema | [porecatu/terminal](https://github.com/porecatu/terminal) |
| **Relay** | Servidor auto-hospedado que encaminha bytes cifrados. Não tem as chaves, não guarda nada | [porecatu/relay](https://github.com/porecatu/relay) |
| **Cliente** | O app mobile (Android e iOS, Expo). Desenha a grade e pede ações | [porecatu/remote](https://github.com/porecatu/remote) |
| **Protocolo** | Este repositório: especificação, decisões, vetores de teste e implementações de referência | [porecatu/protocol](https://github.com/porecatu/protocol) |

## Especificação

Comece pela [visão geral](docs/especificacao/00-visao-geral.md).

1. [00 — Visão geral](docs/especificacao/00-visao-geral.md): partes, princípios, camadas, glossário
2. [01 — Transporte e relay](docs/especificacao/01-transporte-e-relay.md): WebSocket sobre TLS, envelope do relay, `host_id`
3. [02 — Handshake e pareamento](docs/especificacao/02-handshake-e-pareamento.md): chaves, QR code, Noise `XX` e `IK`, registros
4. [03 — Mensagens](docs/especificacao/03-mensagens.md): workspace, grade, input, ações, erros (CBOR + CDDL)
5. [04 — Versionamento](docs/especificacao/04-versionamento.md): as três versões e como mudar a especificação
6. [Modelo de ameaça](docs/especificacao/modelo-de-ameaca.md)
7. [Vetores de teste](docs/especificacao/vetores-de-teste/README.md)

## Documentação do projeto

- [Visão de produto do protocolo](docs/prd/prd-000-visao-de-produto.md)
- [Arquitetura](docs/arquitetura.md): camadas e implementações de referência
- [Decisões (ADRs)](docs/adr/0000-template.md)
- [Roadmap](docs/roadmap.md)
- [Como contribuir](CONTRIBUTING.md) · [Changelog](CHANGELOG.md)

## Licença

[GPL-3.0-or-later](LICENSE), como os outros três projetos ([ADR-0006](docs/adr/0006-licenca-gpl.md)).
