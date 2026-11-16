# 04 — Versionamento

**Versão da especificação:** 0.3 (rascunho)

Há três versões independentes, e misturá-las é a confusão que este documento existe para evitar.

| Versão | O que versiona | Onde aparece | Quem a sobe |
|---|---|---|---|
| **Do envelope do relay** | Camada 2 ([01](01-transporte-e-relay.md)) | Caminho da URL: `/v0/host`, `/v0/client/…` | Mudança no que o relay entende |
| **Do protocolo** `[major, minor]` | Camadas 3 e 4 ([02](02-handshake-e-pareamento.md), [03](03-mensagens.md)) | Prólogo Noise (só o `major`), `v` do QR, `hello`/`welcome` | Mudança no que host e cliente trocam |
| **Da especificação** | Este texto | Cabeçalho de cada documento | Qualquer mudança de texto |

As implementações (Porecatu, Relay, Remote, e o crate e o pacote de referência deste repositório) têm **versões semânticas próprias**, que não se confundem com as de cima.

## Regras do protocolo

- **`major` diferente não conversa.** O prólogo Noise carrega o `major`, então um handshake entre majors diferentes falha antes de qualquer mensagem; o cliente que conseguir distinguir esse caso (pelo `v` do QR, ou pela falha) mostra "atualize o app" ou "atualize o Porecatu".
- **`minor` é aditivo.** Uma versão menor só pode: acrescentar chave opcional a um mapa; acrescentar mensagem nova (mandada só a quem anunciou o `minor` dela); acrescentar valor novo a um enum **que o receptor antigo nunca recebe** (porque o remetente sabe o `minor` do outro). Nunca remove, renomeia ou muda o tipo de nada.
- **Negociação:** o cliente anuncia `[major, minor]` no `hello`; o host responde `welcome` com o mesmo `major` e o **menor** dos dois `minor`, e os dois lados se restringem a ele pelo resto da sessão.
- **Enquanto `major = 0`**, qualquer mudança pode ser incompatível, como no semver: `0.1` e `0.2` podem não conversar. É o período de rascunho; implementações de `major = 0` **DEVEM** exigir `minor` igual. O `major` passa a `1` quando os três projetos publicarem uma versão estável juntos, e daí em diante vale a regra aditiva acima.

## Regras do envelope do relay

- Mudança incompatível no envelope sobe o caminho (`/v1/`). O relay **PODE** servir dois caminhos ao mesmo tempo durante uma transição.
- O relay não conhece a versão do protocolo: um relay `/v0/` serve hosts e clientes de qualquer `[major, minor]`.

## Mudar esta especificação

1. Mudança de **decisão** (o que o protocolo faz) começa num ADR deste repositório.
2. O texto da especificação muda no mesmo PR, com a versão da especificação no cabeçalho.
3. Se a mudança toca o fio, o `[major, minor]` sobe, os vetores de teste ganham casos novos e o [CHANGELOG](../../CHANGELOG.md) registra.
4. As implementações (Porecatu, Relay, Remote) acompanham em PRs próprios, citando a versão.
