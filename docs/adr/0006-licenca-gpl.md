# ADR-0006 — GPL-3.0-or-later nos quatro projetos, com DCO

**Status:** Aceito
**Data:** 2026-11-15
**Relacionados:** [ADR-0005](0005-implementacoes-de-referencia.md); ADR-0010 do Porecatu ([licenciamento](https://github.com/porecatu/terminal/blob/main/docs/adr/0010-licenciamento.md))

## Contexto

O Porecatu é GPL-3.0-or-later. Os três projetos novos (Protocol, Relay e Remote) precisam de licença antes do primeiro arquivo de código. As forças:

- **Coerência**: o crate do protocolo é linkado no Porecatu; o Relay e o Remote são partes do mesmo produto.
- **Serviço de rede**: o Relay roda num servidor; a GPL não obriga quem o opera para terceiros a publicar modificações (a AGPL obrigaria).
- **Lojas de aplicativos**: a App Store da Apple impõe termos de uso ao usuário final que têm atrito conhecido com a GPL quando o app contém contribuições de terceiros que não autorizaram a distribuição ali. O Google Play não tem o mesmo problema.
- **Reuso do protocolo**: licença permissiva no protocolo deixaria qualquer um implementar clientes fechados.

O dono decidiu: **tudo GPL-3.0-or-later**, com o atrito da App Store como risco aceito.

## Decisão

**Os quatro projetos — Porecatu, Protocol, Relay e Remote — são GPL-3.0-or-later. Contribuições entram com sign-off do Developer Certificate of Origin (DCO).**

1. `LICENSE` em cada repositório é a cópia verbatim da FSF, a mesma do Porecatu (sha256 `3972dc9744f6499f0f9b2dbf76696f2ae7ad8af9b23dde66d6af86c9dfb36986`), e nunca é editado.
2. Todo arquivo de código e script começa com `SPDX-License-Identifier: GPL-3.0-or-later`.
3. Dependência nova só com licença compatível com GPLv3 (MIT, Apache-2.0, BSD, ISC, MPL-2.0, …). `snow`, `ciborium`, `@noble/*` e `cbor-x` são MIT ou Apache-2.0.

   > **Nota de implementação (2026-11-16):** o `cbor-x` não é mais dependência (o CBOR é escrito à mão nos dois lados, [ADR-0008](0008-especificacao-0-2.md)). As licenças aceitas no crate são conferidas no CI pelo `cargo deny` (`rust/deny.toml`); no pacote TS, a conferência de licenças ainda não está no CI.
4. **DCO** (`git commit -s`) em todo commit dos projetos novos. O DCO não transfere direitos; registra a origem de cada contribuição, o que mantém possível, no futuro, ao menos saber quem teria de autorizar uma exceção para a App Store.
5. A **especificação em prosa** (os `.md`) está sob a mesma licença; implementar o protocolo a partir dela, sem copiar código, não é obra derivada do código de referência.

## Alternativas consideradas

- **AGPL-3.0 no Relay, GPL no app, MIT/Apache no protocolo.** Fecharia a lacuna de serviço de rede e facilitaria implementações de terceiros. Rejeitada pelo dono em favor da uniformidade; como o relay é auto-hospedado e cego, quem o modifica e o opera não ganha acesso a conteúdo de ninguém.
- **Tudo MIT OR Apache-2.0 nos projetos novos.** Sem atrito com lojas. Rejeitada: permitiria um app fechado sobre o protocolo e o código do Porecatu Remote, contra a linha do projeto.
- **GPL com exceção explícita para a App Store** (cláusula adicional da seção 7 da GPLv3). Resolveria o atrito. Adiada: exige decidir o texto da exceção e colher a concordância de cada contribuidor; o DCO mantém esse caminho aberto.
- **CLA com cessão de direitos.** Daria ao mantenedor liberdade de relicenciar. Rejeitada: afasta contribuições e é desproporcional ao tamanho do projeto.

## Consequências

### Positivas

- Licença única, sem análise de compatibilidade entre os projetos.
- Nenhum derivado fechado do app, do relay ou das implementações de referência.

### Negativas

- Publicar o Remote na App Store depende de o mantenedor ser autor (ou ter autorização) de todo o código distribuído lá; contribuições externas no app complicam isso.
- Operadores que modificam o Relay não são obrigados a publicar as mudanças.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Apple recusar ou retirar o app por conflito de licença | Média | Alto | DCO para rastrear autoria; Android e distribuição por TestFlight/ad hoc não dependem disso; exceção da seção 7 como plano B |
| Dependência JS transitiva com licença incompatível | Média | Médio | Verificação de licenças no CI (`cargo deny`, checagem de licenças do `pnpm`) |
