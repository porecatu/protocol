# ADR-0008 — Especificação 0.2: codificação estrita, sem ponto flutuante, vetores tipados e as lacunas achadas antes do código

**Status:** Aceito
**Data:** 2026-11-15
**Relacionados:** [ADR-0002](0002-noise-para-a-sessao-ponta-a-ponta.md), [ADR-0003](0003-cbor-com-schema-em-cddl.md), [ADR-0004](0004-websocket-sobre-tls.md), [ADR-0005](0005-implementacoes-de-referencia.md)
**Supersedes:** ADR-0003, o uso do `cbor-x` para codificar e o `float32` em `ratio` (**parcial**: o TS codifica com um codificador determinístico próprio e usa o `cbor-x` só para decodificar; nenhuma mensagem tem ponto flutuante); ADR-0005, o envelope TS "só do lado do cliente" (**parcial**: as duas implementações têm os dois papéis)

## Contexto

Antes da primeira linha de código, a validação dos requisitos dos três projetos (protocolo, relay e app) encontrou pontos em que a especificação 0.1 deixava as duas implementações de referência gerarem bytes diferentes, ou em que um consumidor não tinha como cumprir o texto:

- `ratio: float32` contradiz a codificação determinística da RFC 8949 §4.2.1 (que exige o float mais curto que preserva o valor): `ciborium` gera f16 para 0,5, o `cbor-x` gera f64 por padrão, e o CDDL `float32` só aceita 4 bytes. Três fontes, três bytes.
- O `cbor-x` não é determinístico por padrão (ordem de inserção das chaves, cabeçalho de mapa fixo, extensão `useRecords`) e acrescenta a tag 64 a `Uint8Array` **quando `Buffer` existe** — ou seja, no Node e não no Hermes: os testes passariam e o app mandaria outros bytes.
- A rigidez do decodificador não estava escrita: as duas implementações aceitariam e recusariam entradas diferentes.
- O formato dos vetores misturava notação de diagnóstico com objeto JSON, não distinguia `bstr` de `tstr` e não tinha formato para sessões.
- Os ids do protocolo recomeçam quando o host reinicia, e o app, ao reconectar, poderia digitar na aba errada.
- `seq` "reinicia a cada snapshot" deixava um `grid-ack` antigo contar contra a janela nova.
- Lacunas menores: ciclo e erros do envelope no relay, códigos de fechamento, URI de pareamento sem forma exata, URL de presença, falha de handshake, DH nulo, erro sem id de pedido, regra CDDL com nome de tipo do prelúdio.

## Decisão

**A especificação vai à versão 0.2 (texto), mantendo a versão de fio `[0, 1]`, que ainda não foi congelada por nenhuma implementação publicada.** As mudanças:

1. **Nenhum ponto flutuante no fio.** `ratio` vira inteiro `1..65535`, fração do primeiro filho em 65536 avos ([03](../especificacao/03-mensagens.md) §4.1).
2. **Codificação e decodificação estritas** ([03](../especificacao/03-mensagens.md) §1.1), valendo também para o envelope do relay: quem manda usa a forma determinística; quem recebe recusa forma não canônica, chave duplicada, bytes sobrando, tags, floats, valores simples além de `true`/`false`/`null`, inteiros fora de ±(2^53 − 1), aninhamento além de 32 e arrays posicionais com aridade errada. Chave desconhecida continua ignorada, depois da conferência.
3. **No TS, o codificador é próprio** (determinístico, umas duzentas linhas), e o `cbor-x` fica só para decodificar, com a conferência por recodificação. No Rust, o `ciborium` segue, com o mesmo procedimento.

   > **Nota de implementação (2026-11-16):** o código foi além deste item. Os **dois lados** escrevem à mão também o **decodificador**, estrito, que recusa na leitura o que §1.1 de 03 manda recusar, em vez de decodificar com biblioteca e conferir por recodificação (no TS, o caminho do `cbor-x` deixava passar `BigInt`, `Buffer` do Node, chaves numéricas reordenadas e tags convertidas antes da comparação). O `cbor-x` saiu do `package.json`; o `ciborium` não é chamado pelo crate. As regras do fio deste ADR não mudam, e os vetores continuam o árbitro.
4. **`host_instance`** (16 bytes aleatórios por processo do host) no `welcome`; ids só são comparáveis entre sessões com o mesmo valor ([03](../especificacao/03-mensagens.md) §3).
5. **`seq` monotônico por assinatura**, nunca reinicia; snapshot e diff compartilham a sequência; `seq` fora de ordem é erro de protocolo, sem recuperação local.
6. **Resultado de ação só em `action-result`**; `error` ganha `pane` opcional para correlacionar erros de mensagens sem id.
7. **URI de pareamento com forma exata** (ordem dos parâmetros, percent-encoding de tudo fora do conjunto *unreserved* com hexadecimal maiúsculo, normalização do `relay`, nome em valores escalares, parâmetros desconhecidos ignorados e repetidos recusados) ([02](../especificacao/02-handshake-e-pareamento.md) §3.1).
8. **Falha de handshake e DH nulo** definidos: o host fecha com `host-rejected`; as duas partes recusam resultado DH de 32 zeros ([02](../especificacao/02-handshake-e-pareamento.md) §2).
9. **Envelope:** erros do host e do cliente, canal nunca aberto, sem eco de `channel-closed`, limites por canal com o motivo ao host, `1001` no desligamento, `max_hosts` com `4003`, pong perdido sem código, URL de presença derivada da URL base ([01](../especificacao/01-transporte-e-relay.md)).
10. **Vetores em JSON tipado** (`{"bstr": …}`, `{"map": [[k, v], …]}`), com `raiz`, `reencode_hex`, `erro {camada, codigo}`, formato de sessão em passos, cacophony como vem mais vetores próprios com o prólogo do Porecatu, e tabela de casos por categoria ([vetores-de-teste](../especificacao/vetores-de-teste/README.md)).
11. **As duas implementações têm os dois papéis** (cliente e host) e as duas famílias de envelope: o teste cruzado e o desktop falso do app precisam do host em TS.
12. A regra CDDL `text` (mensagem 31) passa a se chamar `text-input`; nomes de regra não vão ao fio.

## Alternativas consideradas

### Manter `float32` como exceção à forma mais curta

Escrever "float32 sempre" e codificar à mão nos dois lados. Funciona, mas deixa um tipo que existe por um campo só, e o CDDL e as bibliotecas continuariam brigando com ele. Um inteiro resolve o mesmo problema sem exceção.

### `cbor-x` com opções explícitas para codificar

`useRecords: false`, `variableMapSize: true`, `tagUint8Array: false` e chaves ordenadas pelo chamador. Depende de que todas as opções continuem fazendo o que fazem hoje em cada versão do `cbor-x`, e o comportamento que varia com o `Buffer` mostra o risco. Um codificador próprio, pequeno e testado pelos vetores, é mais barato de confiar.

### Deixar a rigidez a cargo de cada implementação

Seria aceitar que o Rust recuse o que o TS aceita. Os vetores não teriam como arbitrar.

## Consequências

### Positivas

- As duas implementações têm uma regra só para cada byte, e os vetores conseguem prová-lo.
- O app não reaproveita id de um processo do host em outro.

### Negativas

- Um codificador CBOR a mais para manter no TS.
- A decodificação custa uma recodificação para conferir a forma canônica.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| A conferência por recodificação pesar na grade a 30 mensagens/s | Baixa | Médio | As mensagens de grade são pequenas (diffs); medir na integração com o app |
| Alguma mudança desta leva ter sido pequena demais e voltar | Média | Baixo | Versão de fio ainda `[0, 1]` e não congelada; mudar agora é barato |
