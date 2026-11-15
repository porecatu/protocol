# Como contribuir

Obrigado pelo interesse. Este repositório é o **contrato** entre três projetos; por isso o fluxo é mais rígido do que num projeto comum.

## Antes de abrir um PR

- Leia o [CLAUDE.md](CLAUDE.md) (convenções) e a [visão geral da especificação](docs/especificacao/00-visao-geral.md).
- Mudança de **comportamento do protocolo** começa por uma issue ou por um ADR em `docs/adr/`, não pelo código.

## Fluxo de mudança na especificação

1. **ADR** com a decisão (próximo número; veja o [índice](docs/adr/0000-template.md)).
2. **Texto da especificação** atualizado no mesmo PR, com a versão no cabeçalho dos documentos tocados.
3. Mexeu no fio: sobe `[major, minor]` ([04 — Versionamento](docs/especificacao/04-versionamento.md)), acrescenta **vetores de teste** e registra no [CHANGELOG](CHANGELOG.md).
4. As implementações de referência (quando existirem) passam nos vetores novos no mesmo PR.
5. `python scripts/verify-docs.py` passa.

## Commits

- [Conventional Commits](https://www.conventionalcommits.org), mensagem em português do Brasil.
- **Sign-off obrigatório** (Developer Certificate of Origin, <https://developercertificate.org>): `git commit -s`. O sign-off declara que você tem o direito de contribuir o código sob a licença do projeto.

## Licença

Contribuições entram sob **GPL-3.0-or-later** ([LICENSE](LICENSE)). Todo arquivo de código ou script novo começa com o cabeçalho SPDX:

```
// SPDX-License-Identifier: GPL-3.0-or-later
```
