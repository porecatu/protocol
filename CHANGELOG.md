# Changelog

Mudanças da especificação e das implementações de referência. Formato inspirado no [Keep a Changelog](https://keepachangelog.com/pt-BR/1.1.0/). A versão de fio (`[major, minor]`) e a da especificação são explicadas em [04 — Versionamento](docs/especificacao/04-versionamento.md).

## [Não publicado]

### Especificação 0.1 (rascunho) — 2026-11-15

- Primeira redação: transporte e envelope do relay (`/v0/`), chaves e `host_id`, pareamento por QR com Noise `XX`, conexões com Noise `IK`, registros com fragmentação e rekey, mensagens de aplicação em CBOR com schema em CDDL (workspace, perfis, tema, grade com snapshot/diff e controle de fluxo, scrollback, input semântico, ações com confirmação, erros tipados), modelo de ameaça e formato dos vetores de teste.
- Versão de fio proposta: `[0, 1]`, ainda não congelada.
- ADR-0001 a ADR-0007.
