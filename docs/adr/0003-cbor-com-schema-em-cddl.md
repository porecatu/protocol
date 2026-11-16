# ADR-0003 — Mensagens em CBOR determinístico, com schema em CDDL e vetores de teste

**Status:** Aceito
**Data:** 2026-11-15
**Relacionados:** [ADR-0005](0005-implementacoes-de-referencia.md), [01 — Transporte e relay](../especificacao/01-transporte-e-relay.md) §4, [03 — Mensagens](../especificacao/03-mensagens.md), [04 — Versionamento](../especificacao/04-versionamento.md), [vetores de teste](../especificacao/vetores-de-teste/README.md)

## Contexto

O protocolo precisa de um formato para o envelope do relay e para as mensagens de aplicação. As forças:

- **Volume.** A grade é a maior parte do tráfego: um painel de 200×50 com cor e estilo, a cada diff, por rede móvel. O formato precisa ser compacto e barato de codificar no host, que faz isso na própria thread do espelhamento.
- **Duas linguagens.** Rust no host e no relay, TypeScript no app. Os dois lados têm de produzir e aceitar os mesmos bytes.
- **Evolução aditiva.** As versões menores acrescentam campos e mensagens sem quebrar quem é mais velho ([04](../especificacao/04-versionamento.md)).
- **Sem etapa de geração de código**, se possível: três repositórios gerando tipos de um schema compartilhado é uma dependência de build a mais em cada um.

## Decisão

**Toda mensagem — envelope do relay e aplicação — é um item CBOR (RFC 8949) em codificação determinística; o schema é escrito em CDDL (RFC 8610) dentro da especificação, e a conformidade é verificada por vetores de teste. Rust usa `ciborium` (com `serde`); TypeScript usa `cbor-x`.**

1. **Forma**: `[tipo, corpo]`, `tipo` inteiro, `corpo` mapa com chaves de texto curtas. Nos itens quentes da grade (`span`, `style`, `line`), arrays posicionais em vez de mapas, para não repetir nomes de chave milhares de vezes.
2. **Determinística** (§4.2.1 da RFC 8949): inteiros e comprimentos na forma mais curta, mapas com chaves ordenadas. Torna os vetores comparáveis byte a byte.
3. **Chave desconhecida ignorada, tipo desconhecido recusado**: é o que dá a regra aditiva das versões menores.
4. **CDDL é o schema normativo**, em blocos dentro de [03](../especificacao/03-mensagens.md) e [02](../especificacao/02-handshake-e-pareamento.md); a partir da P1, consolidado em `schema/porecatu-remote.cddl` e conferido no CI contra os vetores.
5. **Os tipos são escritos à mão** em cada implementação de referência, não gerados, e os vetores garantem que batem com o CDDL.

> **Revisão parcial ([ADR-0008](0008-especificacao-0-2.md)):** sem ponto flutuante no fio (`ratio` virou inteiro) e o `cbor-x` deixou de codificar no TS.
>
> **Nota de implementação (2026-11-16):** nenhuma das bibliotecas citadas acima é usada. O codificador determinístico e o decodificador estrito são **escritos à mão nos dois lados** (`rust/src/cbor.rs` e `ts/src/cbor/`); o `cbor-x` foi removido do pacote TS, e o `ciborium` saiu das dependências do crate (só a ferramenta `vectors` o tem, trazido pelo `cddl`). A decisão — CBOR determinístico, CDDL normativo, vetores como árbitro — não muda. O schema consolidado existe em `schema/porecatu-remote.cddl`, gerado dos blocos da especificação por `scripts/extract-cddl.py`, e `vectors check` confere os vetores contra ele.

## Alternativas consideradas

- **Protocol Buffers.** Schema `.proto` como fonte única, tipos gerados para Rust (`prost`) e TS, evolução de campos bem resolvida. Rejeitada: geração de código em três repositórios (com `protoc` ou plugins no build de cada um, inclusive no Expo), e os tipos gerados vazam para o código de quem consome; a regra de evolução que o Protobuf dá é obtida em CBOR com "chave desconhecida é ignorada". O ganho de tamanho do Protobuf sobre CBOR com arrays posicionais nas partes quentes é pequeno.
- **JSON.** Legível e trivial de depurar. Rejeitada pelo volume: sem binário, cada diff de grade fica várias vezes maior, e texto de terminal com caracteres de controle precisa de escape; codificar JSON por diff também custa mais CPU no host.
- **MessagePack.** Muito parecido com CBOR em tamanho e suporte. Rejeitada por CBOR ter padrão IETF de schema (CDDL), codificação determinística especificada e ser a escolha natural ao lado de RFCs de protocolo; a diferença prática é pequena.
- **Formato binário próprio.** Máximo controle de tamanho. Rejeitada: duas implementações de parser à mão, sem ferramenta de diagnóstico, para ganhar pouco sobre CBOR.

## Consequências

### Positivas

- Mensagens compactas sem geração de código; bibliotecas maduras nas duas linguagens.
- Notação de diagnóstico do CBOR (RFC 8949 §8) dá vetores de teste legíveis.
- Evolução aditiva por regra simples.

### Negativas

- Tipos escritos à mão em duas linguagens podem divergir do CDDL; só os vetores pegam.
- CDDL tem pouca ferramenta de validação em TS; a validação forte fica no lado Rust e nos vetores.
- Arrays posicionais nas partes quentes são menos autoexplicativos; a especificação compensa com comentários em cada campo.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| `cbor-x` e `ciborium` divergirem na codificação determinística (ordem de chaves, float) | Média | Médio | Vetores com `cbor_hex` exato; floats só em `ratio` (float32 fixo); teste de ida e volta nos dois |
| Mensagem maliciosa enorme ou profundamente aninhada | Média | Médio | Teto de 16 MiB por mensagem; limites de profundidade e de tamanho de array nos decodificadores das implementações de referência |
| Tipo escrito à mão divergir do schema | Média | Médio | CI valida os vetores contra o CDDL consolidado (P1) e contra as duas implementações |
