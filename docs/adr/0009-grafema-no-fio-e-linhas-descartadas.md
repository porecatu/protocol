# ADR-0009 — Grafema composto marcado no fio e contador de linhas descartadas do histórico

**Status:** Aceito
**Data:** 2026-11-16
**Relacionados:** [ADR-0003](0003-cbor-com-schema-em-cddl.md), [ADR-0008](0008-especificacao-0-2.md)
**Supersedes:** ADR-0008, a forma do `span` e do scrollback da especificação 0.2 (**parcial**: só o `text` do span e o `dropped` da grade e da página)

## Contexto

Ao implementar o modelo da grade e o cache de scrollback do Porecatu Remote, duas lacunas da especificação 0.2 apareceram:

1. **Grafema composto.** A 03 §5.2 manda o host pôr todo grafema de vários code points sozinho num span, mas o fio não distingue esse span de um span comum: `"é"` (`e` + acento combinante) e `"ab"` chegam iguais, um texto de dois code points. O cliente teria de adivinhar — o app escreveu uma heurística por classes Unicode (combinantes, ZWJ, seletores de variação, indicadores regionais), que falha em qualquer grafema fora delas e depende de suporte a classes Unicode no motor de regex do Hermes.
2. **Histórico com teto.** O scrollback do host tem tamanho máximo. Cheio, cada linha nova empurra a mais antiga para fora e `history` fica constante. O cliente, que corrige o deslocamento das páginas pela variação de `history`, passa a ter o cache deslocado em silêncio.

A versão de fio `[0, 1]` ainda não foi congelada por nenhuma implementação publicada; mudar agora é barato.

## Decisão

**O `text` de um span é texto (um code point por célula) ou um array de exatamente um texto (um único grafema numa célula); e `grid-snapshot`, `grid-diff` e `scrollback-page` levam o contador `dropped` de linhas já descartadas do topo do histórico, que dá a cada linha um índice absoluto estável.**

- `span = [col, width, style, text: tstr / [tstr]]`. Array vazio, com mais de um elemento ou com texto vazio é fora do domínio. O host nunca põe grafema de vários code points num span comum.
- `dropped: uint`, obrigatório nas três mensagens, monotônico por painel; zera só quando o histórico do painel é limpo, e então vem um `grid-snapshot`. Índice absoluto = `dropped + (history − 1 − offset)`.
- Especificação 0.3 (texto); versão de fio continua `[0, 1]`, não congelada. Vetores, schema e as duas implementações acompanham.

## Alternativas consideradas

### Grafema: quinto elemento no span ou largura especial

Um elemento opcional quebraria a regra de aridade exata dos arrays posicionais; um `width` 3/4 para "grafema" misturaria duas informações num campo. O array de um elemento é auto-descritivo e custa um byte.

### Grafema: segmentação no cliente

Exigiria a tabela de grafemas do Unicode no app (o `Intl.Segmenter` não existe no Hermes) e a mesma versão de Unicode dos dois lados. O host já segmentou; basta dizer o resultado.

### Histórico: o host avisar a cada descarte

Uma mensagem por linha descartada seria ruído; um contador cumulativo na mensagem que já vai é suficiente e tolera perda de qualquer passo intermediário.

### Histórico: o cliente invalidar o cache sempre que `history` estiver no teto

O cliente não sabe qual é o teto; e invalidar tudo a cada linha nova com o histórico cheio é o mesmo que não ter cache.

## Consequências

### Positivas

- O cliente desenha qualquer grafema sem heurística e sem tabela Unicode.
- O cache de scrollback do cliente é chaveado por índice absoluto e nunca desloca.

### Negativas

- O host precisa contar as linhas descartadas do histórico de cada painel (o motor do desktop não expõe esse número pronto: deriva do total de linhas empurradas menos o tamanho do histórico).

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| O host errar a contagem de `dropped` | Média | Médio | Teste no desktop que enche o histórico além do teto e confere os índices absolutos contra o conteúdo |
