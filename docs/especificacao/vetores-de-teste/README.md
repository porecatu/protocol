# Vetores de teste

**Status:** formato definido; os vetores nascem junto com as implementações de referência ([ADR-0005](../../adr/0005-implementacoes-de-referencia.md)).

O protocolo tem duas implementações de referência em linguagens diferentes (Rust e TypeScript). Especificação em prosa não basta para as duas concordarem byte a byte: os vetores são o árbitro. Toda mudança no fio entra com vetor novo, e o CI deste repositório roda os vetores contra as duas implementações.

## Organização

```
vetores-de-teste/
├── README.md                 este arquivo
├── envelope/                 camada 2: envelopes do relay
├── noise/                    camada 3: handshakes XX e IK com chaves fixas, registros, fragmentação, rekey
├── mensagens/                camada 4: uma pasta por tipo de mensagem
└── sessoes/                  conversas inteiras, do hello ao goodbye
```

## Formato de um vetor

Cada vetor é um arquivo JSON (JSON porque qualquer linguagem o lê sem dependência e o diff é legível):

```json
{
  "descricao": "grid-diff com um grafema ZWJ sozinho num span",
  "versao": [0, 1],
  "valor": { "...": "a mensagem em notação de diagnóstico CBOR (RFC 8949 §8)" },
  "cbor_hex": "82171a...",
  "valido": true
}
```

- **`valido: true`**: decodificar `cbor_hex` dá `valor`, e codificar `valor` dá exatamente `cbor_hex` (codificação determinística).
- **`valido: false`**: o decodificador **DEVE** recusar, e o vetor diz com que `error-code` (campo `erro`).
- Vetores de Noise trazem as chaves privadas fixas, os efêmeros fixos e cada mensagem de handshake em hexadecimal, no formato dos vetores do próprio Noise (`cacophony`), mais o prólogo do Porecatu.

## Casos obrigatórios por mensagem

1. O mínimo válido (só chaves obrigatórias).
2. O completo (todas as chaves opcionais).
3. Uma chave desconhecida, que **DEVE** ser ignorada.
4. Uma chave obrigatória ausente, que **DEVE** ser recusada.
5. Para mensagens de grade: largura dupla, grafema de vários code points, linha vazia, estilo com as três formas de cor.
