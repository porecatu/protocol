# 00 — Visão geral do protocolo

**Versão da especificação:** 0.6 (rascunho)
**Data:** 2026-11-15
**Status:** Rascunho. As duas implementações de referência deste repositório (crate Rust `porecatu-protocol` e pacote TypeScript `@porecatu/protocol`) implementam este texto até a 0.5, com a versão de fio `[0, 1]`; o que a 0.6 acrescenta (sessões salvas, §4.5 e §7 de [03](03-mensagens.md), [ADR-0012](../adr/0012-sessoes-nomeadas-pelo-cliente.md)) ainda não está implementado; nenhuma foi publicada ainda, e a primeira implementação que for publicada congela `[0, 1]` ([04 — Versionamento](04-versionamento.md)). A versão de fio não é a versão deste texto (0.6).

Este documento é a **especificação normativa** do protocolo de acesso remoto do Porecatu. As três implementações que o falam — o Porecatu desktop, o Porecatu Relay e o Porecatu Remote — seguem o que está escrito aqui, e divergência entre o texto e uma implementação é bug da implementação ou, se o texto estava errado, correção do texto **neste repositório primeiro**.

As palavras **DEVE**, **NÃO DEVE**, **DEVERIA** e **PODE** têm o peso de MUST, MUST NOT, SHOULD e MAY da RFC 2119.

## O problema

O Porecatu é um emulador de terminal de desktop cujo diferencial é a gestão de muitos terminais: abas, grupos, painéis, sessões. O usuário quer alcançar essa sessão viva a partir do celular — ver o que está rodando, digitar numa aba, abrir uma aba nova, reorganizar — sem expor o desktop à internet e sem confiar o conteúdo dos terminais a um servidor.

## As três partes

```
+--------------------+        wss://        +------------------+        wss://        +--------------------+
|  Porecatu desktop  | <------------------> |  Porecatu Relay  | <------------------> |  Porecatu Remote   |
|  (host)            |   canal do host      |  (auto-hospedado)|   canal do cliente   |  (cliente, mobile) |
|  dono do estado    |                      |  cego: roteia    |                      |  só desenha e pede |
+--------------------+                      +------------------+                      +--------------------+
          ^                                                                                     ^
          +--------------------- sessão Noise ponta a ponta (cifrada) --------------------------+
```

| Parte | Papel | Repositório |
|---|---|---|
| **Host** | O processo do Porecatu desktop. É a **única fonte da verdade**: workspace, PTYs, motor VT, tema. Conecta-se ao relay por iniciativa própria (nunca escuta porta) | [porecatu/terminal](https://github.com/porecatu/terminal) |
| **Relay** | Servidor auto-hospedado que encaminha bytes entre host e clientes. **Cego**: não tem as chaves da sessão, não interpreta mensagens de aplicação e não guarda nada em disco | [porecatu/relay](https://github.com/porecatu/relay) |
| **Cliente** | O app Porecatu Remote (Android e iOS). Não guarda estado de workspace: recebe a árvore e a grade do host, desenha, e manda input e pedidos de ação | [porecatu/remote](https://github.com/porecatu/remote) |

## Princípios

1. **O host é a fonte da verdade.** O cliente nunca decide o estado do workspace nem interpreta sequências VT. Ele recebe a grade já resolvida pelo motor do host e desenha células.
2. **O relay é cego.** Tudo que é de aplicação — grade, teclas, nomes de aba, diretórios — viaja cifrado de ponta a ponta. O relay vê só o que precisa para rotear ([01 — Transporte e relay](01-transporte-e-relay.md)) e o que a rede inevitavelmente revela (tamanho e momento dos quadros); isso está no [modelo de ameaça](modelo-de-ameaca.md).
3. **Sem contas.** Identidade é chave: o host tem um par de chaves, cada aparelho tem o seu, e o vínculo entre os dois nasce de um pareamento presencial por QR code ([02 — Handshake e pareamento](02-handshake-e-pareamento.md)).
4. **Ação remota é o mesmo gesto do desktop.** Toda ação que o cliente pede é executada pelo host pelo mesmo caminho do gesto local, com as mesmas regras e confirmações ([03 — Mensagens](03-mensagens.md) §7).
5. **O desktop dita o tamanho.** Um PTY tem um tamanho só; o cliente adapta zoom e rolagem, e nunca redimensiona o terminal do host.
6. **Nenhuma prosa no fio.** Erros e motivos são **códigos tipados**; o texto que o usuário lê é composto por quem o mostra, no idioma dele. É a mesma regra do Porecatu (texto de interface só nasce na camada de UI).
7. **Ocioso custa zero.** Painel sem mudança não gera mensagem; o host só lê o motor de um painel que algum cliente assina.

## Camadas

| # | Camada | Quem enxerga | Documento |
|---|---|---|---|
| 1 | WebSocket sobre TLS (`wss://`), quadros binários | rede, relay | [01](01-transporte-e-relay.md) |
| 2 | Envelope do relay (CBOR): registro do host, abertura e fechamento de canal, dados opacos | relay | [01](01-transporte-e-relay.md) |
| 3 | Sessão Noise (`XX` no pareamento, `IK` depois) e registros cifrados | host e cliente | [02](02-handshake-e-pareamento.md) |
| 4 | Mensagens de aplicação (CBOR, schema em CDDL) | host e cliente | [03](03-mensagens.md) |

## Glossário

| Termo | Significado |
|---|---|
| **host** | O processo do Porecatu desktop com o acesso remoto ligado |
| **cliente** | Uma instância do Porecatu Remote conectada a um host |
| **aparelho** | O par de chaves estáticas de um cliente; é o que o host pareia e revoga |
| **`host_id`** | Endereço opaco do host no relay, derivado da chave de roteamento do host ([01](01-transporte-e-relay.md) §3) |
| **canal** | Uma conexão de cliente encaminhada pelo relay ao host, identificada por um `channel` inteiro escolhido pelo relay |
| **sessão** | A sessão Noise estabelecida dentro de um canal |
| **registro** | Uma unidade cifrada da camada 3 (até 65 535 bytes no fio) |
| **mensagem** | Uma unidade da camada 4, que pode ocupar vários registros |
| **assinatura** | O pedido do cliente para receber a grade de um painel ([03](03-mensagens.md) §5) |

## Índice

- [01 — Transporte e relay](01-transporte-e-relay.md)
- [02 — Handshake e pareamento](02-handshake-e-pareamento.md)
- [03 — Mensagens](03-mensagens.md)
- [04 — Versionamento](04-versionamento.md)
- [Modelo de ameaça](modelo-de-ameaca.md)
- [Vetores de teste](vetores-de-teste/README.md)
