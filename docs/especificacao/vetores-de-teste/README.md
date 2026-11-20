# Vetores de teste

**Versão da especificação:** 0.5 (rascunho)
**Status:** formato definido ([ADR-0008](../../adr/0008-especificacao-0-2.md)); 340 vetores commitados, gerados pela ferramenta Rust `rust/tools/vectors` (todas as pastas menos `sessoes-ts/`) e pelos testes do pacote TypeScript (`sessoes-ts/`), e reproduzidos pelas duas implementações de referência ([ADR-0005](../../adr/0005-implementacoes-de-referencia.md)).

O protocolo tem duas implementações de referência em linguagens diferentes (Rust e TypeScript). Especificação em prosa não basta para as duas concordarem byte a byte: os vetores são o árbitro. Toda mudança no fio entra com vetor novo, e o CI deste repositório roda os vetores contra as duas implementações.

> **Duas versões, não confundir.** O campo `versao` dos vetores é a **versão de fio** `[major, minor]` (hoje `[0, 1]`, a constante `PROTOCOL_VERSION` do crate e do pacote), não a versão deste texto (0.3). A especificação pode mudar de texto sem mudar o fio; o contrário não ([04 — Versionamento](../04-versionamento.md)).

## Organização

```
vetores-de-teste/
├── README.md                 este arquivo
├── identidade/               host_id, impressão digital, URI de pareamento
├── envelope/                 camada 2: envelopes do relay (host e cliente)
├── noise/                    camada 3: cacophony (como vem) e handshakes XX/IK com o prólogo do Porecatu
├── registros/                camada 3: fragmentação, flags, rekey
├── mensagens/<tipo>/         camada 4: uma pasta por mensagem (ex.: mensagens/grid-diff/)
├── sessoes/                  conversas inteiras, do handshake às primeiras mensagens (geradas pelo Rust)
└── sessoes-ts/               conversas inteiras no formato de sessoes/, geradas pelo TypeScript
```

Um arquivo JSON por vetor, nome em `kebab-case` descrevendo o caso (`minimo.json`, `chave-desconhecida.json`, `sem-pane.json`). A exceção é `noise/cacophony-xx-ik.json`, um arquivo com vários vetores.

| Pasta | Arquivos | Vetores | Quem gera | Quem reproduz |
|---|---|---|---|---|
| `identidade/` | 14 | 14 | Rust (`rust/tools/vectors`) | Rust e TS |
| `envelope/` | 38 | 38 | Rust | Rust e TS |
| `mensagens/` | 259 (28 pastas: as 27 mensagens de [03](../03-mensagens.md) §10 e o `pair-request`) | 259 | Rust | Rust e TS |
| `noise/` | 6 | 9 (5 do Porecatu + 4 do cacophony) | Rust; o arquivo do cacophony é copiado como vem | Rust e TS |
| `registros/` | 11 | 11 | Rust | Rust e TS |
| `sessoes/` | 6 | 6 | Rust | Rust e TS |
| `sessoes-ts/` | 3 | 3 | TS (`ts/test/transcripts.test.ts`) | Rust e TS |

## O valor tipado

JSON não distingue `bstr` de `tstr` nem inteiro de outro número com segurança. Por isso o valor CBOR de um vetor é escrito numa **árvore JSON tipada**, com mapeamento fixo:

| CBOR | JSON tipado |
|---|---|
| inteiro (`-(2^53−1) .. 2^53−1`) | número JSON inteiro |
| `tstr` | string JSON |
| `bstr` | `{"bstr": "<hex minúsculo>"}` |
| array | array JSON |
| mapa (chaves de texto) | `{"map": [["chave", <valor>], ...]}` — lista de pares **na ordem canônica**, para preservar a ordem e permitir chaves que colidiriam num objeto |
| `true`, `false`, `null` | `true`, `false`, `null` |

A notação de diagnóstico CBOR (RFC 8949 §8) **pode** vir no campo `diag`, como comentário para humanos; nenhuma ferramenta a lê.

Os formatos do Porecatu abaixo são fechados: campo a mais num vetor é erro do leitor da ferramenta Rust, não campo ignorado. O arquivo do cacophony, que vem de fora, é a exceção.

## Formato de um vetor de valor (envelope, mensagens)

```json
{
  "descricao": "grid-diff com um grafema ZWJ sozinho num span",
  "versao": [0, 1],
  "raiz": "message",
  "valor": [23, {"map": [["pane", 7], ["seq", 3], ["...", "..."]]}],
  "cbor_hex": "82171a...",
  "reencode_hex": null,
  "valido": true,
  "erro": null,
  "diag": "[23, {\"pane\": 7, ...}]"
}
```

- **`raiz`**: a regra CDDL raiz do item (`host-envelope`, `client-envelope`, `handshake-start`, `pair-request`, `message`); é ela que o validador CDDL usa.
- **`valido: true`**: decodificar `cbor_hex` dá `valor`; codificar `valor` dá `cbor_hex`.
- **`reencode_hex`**: só para o caso de chave desconhecida. O decodificador aceita `cbor_hex` (que tem a chave a mais) e, ao recodificar a mensagem **tipada**, produz `reencode_hex` (sem a chave). Nos demais vetores, `null`.
- **`valido: false`**: o decodificador **DEVE** recusar `cbor_hex`; `valor` é `null` se o item nem é CBOR válido. `erro` diz como:

  ```json
  "erro": { "camada": "mensagem", "codigo": "protocol" }
  ```

  `camada` ∈ `envelope` (o `codigo` é a `close-reason` ou o código de fechamento WebSocket que o relay usaria, como texto: `"protocol-error"`, `"4004"`), `registro` (`"decrypt"`, `"reserved-flag"`, `"too-large"`), `handshake` (`"host-key-mismatch"`, `"dh-zero"`, `"decrypt"`, `"timeout"`), `mensagem` (um `error-code` de [03](../03-mensagens.md) §9, quase sempre `"protocol"`; nas sessões, também o nome do que encerra a conversa), `uri` (`"not-porecatu"`, `"missing-field"`, `"duplicate-field"`, `"malformed-field"`, `"version-mismatch"`, `"insecure-relay"`).

## Vetores de identidade

`identidade/` não tem `versao`, `raiz` nem `cbor_hex`: cada vetor é uma função, a entrada e a saída esperada.

```json
{ "descricao": "host_id de uma chave fixa", "funcao": "host_id",
  "entrada": {"bstr": "…32 bytes…"}, "saida": "k3x9…", "valido": true, "erro": null }
```

- `funcao` ∈ `host_id`, `fingerprint`, `pairing_uri_format`, `pairing_uri_parse`.
- `entrada` e `saida` são valores tipados: a chave em `bstr`; os campos da URI como mapa tipado (`pairing_uri_format` recebe o mapa e devolve a URI em texto; `pairing_uri_parse`, o contrário).
- Recusado: `valido: false`, `saida: null` e `erro` com `camada: "uri"`.

## Vetores de Noise (`noise/`)

### Cacophony

`noise/cacophony-xx-ik.json` traz quatro vetores de `Noise_XX_25519_ChaChaPoly_BLAKE2s` e `Noise_IK_25519_ChaChaPoly_BLAKE2s` **copiados como vêm** dos arquivos de teste que o crate `snow` 0.10.0 distribui (`tests/vectors/cacophony.txt`, gerados pela implementação cacophony, e `tests/vectors/snow.txt`), no formato de vetores do Noise. O prólogo e os payloads são os deles, não os do Porecatu. O arquivo tem `formato: "noise-cacophony"`, `source`, `license` (Apache-2.0 OR MIT), `format` e `vectors`; cada vetor tem `protocol_name`, os prólogos e as chaves dos dois lados em hexadecimal e `messages` (`payload` e `ciphertext`, iniciador nos índices pares, primeiro o handshake e depois mensagens de transporte). Servem para provar que as duas implementações são Noise correto. A ferramenta Rust não os gera nem os reescreve. O teste TS lê uma cópia em `ts/test/fixtures/cacophony-xx-ik.json` e confere que os `vectors` dela são iguais aos deste arquivo.

### Handshakes do Porecatu

Os outros cinco arquivos fixam todas as chaves estáticas e efêmeras e guardam cada mensagem do handshake:

```json
{
  "descricao": "XX com o prólogo do Porecatu, chaves e efêmeros fixos; a msg3 leva o pair-request",
  "versao": [0, 1],
  "padrao": "XX",
  "nome": "Noise_XX_25519_ChaChaPoly_BLAKE2s",
  "host_id": "rxlgbt4qzkrrqzg4v3e6kjsprj7j6pmx",
  "prologo_hex": "706f7265…",
  "chaves": {
    "host_estatica": {"bstr": "…privada…"}, "host_efemera": {"bstr": "…"},
    "cliente_estatica": {"bstr": "…"}, "cliente_efemera": {"bstr": "…"}
  },
  "chave_do_host": {"bstr": "…pública…"},
  "mensagens": [
    { "de": "cliente", "payload": {"bstr": ""}, "hex": "34e4…" },
    { "de": "host",    "payload": {"bstr": ""}, "hex": "392d…" },
    { "de": "cliente", "payload": {"bstr": "a365…"}, "hex": "…" }
  ],
  "handshake_hash": "…",
  "quem_falha": null,
  "valido": true,
  "erro": null
}
```

- `padrao` é `XX` (pareamento) ou `IK` (conexão de aparelho pareado); `nome`, o nome Noise completo; `prologo_hex`, o prólogo de [02](../02-handshake-e-pareamento.md) para esse `host_id`.
- `chaves` são as **privadas** (32 bytes cada). `chave_do_host` é a chave pública do host que o cliente recebeu: a `key` do QR no `XX`, a guardada no `IK`. No caso `host-key-mismatch` ela difere da que o host revela.
- `mensagens`, em ordem: `payload` em claro (vazio, exceto a `msg3` do `XX`, que leva o `pair-request` em CBOR) e `hex`, a mensagem Noise.
- `handshake_hash` é o hash com que os dois lados terminam; `null` quando o handshake falha.
- Recusado: `valido: false`, `quem_falha` (`cliente` ou `host`) e `erro` com `camada: "handshake"` (`host-key-mismatch`, `dh-zero`). As mensagens listadas são as que chegam a ser escritas antes da recusa (nenhuma, no DH nulo do cliente).

## Vetores de registros (`registros/`)

Fragmentação, flags e rekey sobre um transporte aberto por um handshake `IK` com as chaves fixas. A mensagem em claro é sempre o padrão `byte i = i mod 251` de um tamanho dado; o vetor guarda o tamanho e o BLAKE2s-256 dela, nunca os bytes.

```json
{
  "descricao": "REKEY forçado por idade: chave de 1000 ms, o handshake termina em 0 ms",
  "versao": [0, 1],
  "host_id": "rxlgbt4qzkrrqzg4v3e6kjsprj7j6pmx",
  "chaves": { "host_estatica": {"bstr": "…"}, "…": "…" },
  "politica": { "max_records": 100000, "max_age_ms": 1000 },
  "handshake_ms": 0,
  "passos": [
    {
      "de": "cliente",
      "now_ms": 1000,
      "texto_claro": { "padrao": "i mod 251", "tamanho": 10, "blake2s": "bb47…" },
      "flags": [2],
      "registros_hex": ["313e…"],
      "registros_blake2s": ["33eb…"],
      "forjado": false
    }
  ],
  "valido": true,
  "erro": null
}
```

- `politica` é a política de rekey das duas sessões (o padrão da especificação é 100 000 registros ou 60 minutos); `handshake_ms`, o instante em que o handshake terminou, dado às duas.
- Cada passo é **uma mensagem** de quem está em `de`, no instante `now_ms`: `flags` traz o byte de flags de cada registro (`0x01` CONTINUA, `0x02` REKEY), lido do texto cifrado e não previsto; `registros_hex`, os registros inteiros; `registros_blake2s`, o BLAKE2s-256 de cada um.
- Mensagens até 65 519 bytes guardam os registros inteiros; acima disso `registros_hex` é `null` e só `registros_blake2s` fica, para os arquivos não crescerem.
- `forjado: true`: os registros foram montados abaixo da camada de registros (flag reservada ligada, bit trocado); só o receptor é reproduzido.
- Recusado: `valido: false` e `erro` com `camada: "registro"`, sempre no **último** passo. Na mensagem acima de 16 MiB quem recusa é o remetente, e o passo não tem registros.

## Vetores de sessão (`sessoes/` e `sessoes-ts/`)

Conversas inteiras: o handshake, os registros de cada mensagem e os quadros WebSocket exatos.

```json
{
  "descricao": "conexão IK de um aparelho que não está na lista: …",
  "versao": [0, 1],
  "host_id": "rxlgbt4qzkrrqzg4v3e6kjsprj7j6pmx",
  "canal": 1,
  "handshake": "IK",
  "chaves": { "host_estatica": {"bstr": "…"}, "…": "…" },
  "token_host": null,
  "pareados": [{"bstr": "3f3e…"}],
  "passos": [
    { "de": "cliente", "now_ms": 0,  "mensagem": null, "registros_hex": ["…msg1…"], "envelope_hex": ["821058…"] },
    { "de": "host",    "now_ms": 10, "mensagem": null, "registros_hex": ["…msg2…"], "envelope_hex": ["830401…"] },
    { "de": "host",    "now_ms": 10, "mensagem": [70, {"map": [["code", 1]]}], "registros_hex": ["…"], "envelope_hex": ["830401…"] },
    { "de": "host",    "now_ms": 11, "mensagem": null, "registros_hex": [], "envelope_hex": ["83060100"] }
  ],
  "resultado": { "estado": "fechada", "erro": { "camada": "mensagem", "codigo": "not-paired" } }
}
```

- `versao` é a versão de fio do **host**; `canal`, o número de canal que o relay deu a esta conversa (o `channel` dos quadros do host); `handshake`, `XX` ou `IK`.
- `token_host` é o token de pareamento que o host guarda, emitido no instante 0 (`bstr` de 16 bytes), ou `null`; `pareados`, as chaves públicas dos aparelhos que o host já pareou. É deles que sai a decisão do host (aceitar, `pair-rejected`, `error not-paired`).
- Cada passo é uma mensagem, uma mensagem de handshake ou o host fechando o canal:
  - **handshake**: `mensagem` é `null` e `registros_hex` traz a mensagem Noise (`msg1`, `msg2`, `msg3`). Exceção: a `msg3` do `XX` tem em `mensagem` o `pair-request` tipado que ela carrega;
  - **mensagem de aplicação**: `mensagem` em claro (valor tipado) e os registros que a levam;
  - **fechamento**: passo do host com `mensagem: null`, `registros_hex: []` e um quadro `close-channel`.
- `envelope_hex` são os quadros que **quem envia** escreve: os do cliente são `data` (`[16, payload]`), os do host são `channel-data` (`[4, canal, payload]`) na conexão dele com o relay; o relay troca um pelo outro mantendo o payload ([01](../01-transporte-e-relay.md) §4). O primeiro quadro do cliente leva um `handshake-start` (`[tipo, msg1]`) como payload.
- `resultado.estado` é `aberta` ou `fechada`; fechada, `erro` tem `camada: "mensagem"` e em `codigo` o que encerrou a conversa: o `code` do `error` (`not-paired`), a mensagem `incompatible` ou o `reason` do `pair-rejected` (`token-invalid`).

Reproduzir uma sessão é rodar os dois lados com as chaves, os efêmeros e os `now_ms` dados e comparar **byte a byte** cada mensagem Noise, registro e quadro; o receptor tem de devolver a `mensagem` do passo, e as respostas do host têm de seguir as regras de [02](../02-handshake-e-pareamento.md) e [03](../03-mensagens.md) (o que ele deve depois de um `pair-request`, de um `IK` desconhecido, de um `hello`).

### `sessoes-ts/`: o sentido TS → Rust do teste cruzado

`sessoes/` prova que o TypeScript reproduz o que o Rust escreve. `sessoes-ts/` fecha o outro sentido: três conversas (`pareamento-aceito`, `conexao-ik-pareado`, `conexao-ik-desconhecido`) gravadas pelo `ClientSession` e pelo `HostChannel` do pacote TypeScript, com chaves, efêmeros e relógio fixos, no mesmo formato de `sessoes/`. Cobrem mensagens que as sessões do Rust não têm: `grid-diff`, `key`, `text-input`, `paste`, `bell`, `unsubscribe`, `action` com `action-result`.

- **Quem gera:** `ts/test/support/transcript.ts`. Na execução normal, `ts/test/transcripts.test.ts` confere que os arquivos commitados são exatamente o que o pacote escreve hoje; mudança de fio no TS reprova o teste em vez de passar despercebida.
- **Quem reproduz:** o crate Rust, em `rust/tests/vectors.rs` (`typescript_transcripts`), nas camadas de handshake e registros e com as máquinas de estado `ClientSession`/`HostChannel`; e o próprio pacote, com as mesmas funções que reproduzem `sessoes/`.
- **A ferramenta Rust não toca nesta pasta** (`generate` só reescreve as pastas dela); `check` confere os arquivos daqui como os de `sessoes/`.

## Gerar e conferir

Do lado Rust (dentro de `rust/`):

```bash
cargo run -p vectors -- generate   # reescreve identidade/, envelope/, mensagens/, noise/ (menos o cacophony), registros/ e sessoes/
cargo run -p vectors -- check      # confere todos os vetores (inclusive sessoes-ts/) contra o crate e o CDDL; sai com 1 se algum falhar
cargo run -p vectors -- schema     # confere que schema/porecatu-remote.cddl é CDDL válido
cargo test --workspace             # reproduz todos os vetores (rust/tests/vectors.rs)
```

O `--workspace` importa: reproduzir os vetores de Noise do Porecatu, os de registros e os de sessão exige a feature `test-vectors` do crate (efêmeros fixos), ligada pela ferramenta `vectors` do mesmo workspace. Sem ela (`cargo test -p porecatu-protocol`), esses casos passam sem reproduzir nada.

`generate` é determinístico (mesmos bytes a cada execução) e confere cada vetor contra o crate antes de gravar; os de Noise, registros e sessão são montados rodando os dois lados no mesmo processo. `check` confere, nos vetores de valor, `cbor_hex` contra `valor` pelo codificador e decodificador estritos do crate e `cbor_hex` (e `reencode_hex`) contra o CDDL na regra `raiz`; nos de Noise, registros e sessão, a consistência interna (hexadecimal, tamanhos, hashes, quadros pelos decodificadores de envelope). Reproduzir handshakes e registros é trabalho de `rust/tests/vectors.rs`.

O schema `schema/porecatu-remote.cddl` não é escrito à mão: `python scripts/extract-cddl.py` o gera dos blocos `cddl` de 01, 02 e 03, e `scripts/verify-docs.py` reprova o arquivo fora de sincronia.

Do lado TypeScript (dentro de `ts/`):

```bash
pnpm test                                                         # reproduz todos os vetores, inclusive sessoes-ts/
WRITE_TRANSCRIPTS=1 pnpm exec vitest run test/transcripts.test.ts  # regrava sessoes-ts/
```

Depois de regravar `sessoes-ts/`, rode `cargo test --workspace` em `rust/`: é ele que prova que o Rust reproduz as transcrições novas. No `pnpm test` normal, o teste que regrava aparece como **pulado** (é o único).

## Casos por categoria

| Categoria | Casos obrigatórios |
|---|---|
| Mensagem com corpo em mapa | (1) mínimo (só obrigatórias); (2) completo (todas as opcionais); (3) chave desconhecida, aceita, com `reencode_hex`; (4) cada chave obrigatória ausente, recusada; (5) item não canônico (chave fora de ordem ou inteiro não mínimo), recusado |
| Mensagens de grade | além dos de cima: largura dupla; grafema de vários code points sozinho num span; linha vazia; as três formas de `color` |
| Envelope (arrays) | (1) cada tipo válido; (2) aridade errada; (3) tipo desconhecido; (4) `close-reason` fora do domínio; (5) bytes sobrando |
| Registros | fragmentação em 2 e em 3 registros; `REKEY`; bit reservado; mensagem acima de 16 MiB |
| Noise | cacophony XX e IK; XX e IK com o prólogo do Porecatu; chave do QR diferente (`host-key-mismatch`); DH nulo |
| Identidade | `host_id` e impressão digital de chaves fixas; URI completa com caracteres fora do conjunto *unreserved*; parâmetro desconhecido ignorado; parâmetro repetido; `relay` inseguro |
| Sessões | pareamento aceito; pareamento recusado (token); conexão `IK` de aparelho pareado; `IK` de aparelho desconhecido (`not-paired`); `incompatible` |

Os casos (3), (4) e (5) das mensagens são **derivados automaticamente** pela ferramenta a partir do caso (1): `chave-desconhecida` acrescenta a chave `"zz"` no lugar canônico; `sem-<chave>` tira cada obrigatória por vez; `nao-canonico` troca duas chaves de lugar.
