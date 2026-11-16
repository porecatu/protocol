#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""
Extrai o schema CDDL consolidado da especificacao.

    python scripts/extract-cddl.py            # (re)gera schema/porecatu-remote.cddl
    python scripts/extract-cddl.py --check    # sai com 1 se o arquivo estiver fora de sincronia

A fonte do schema sao os blocos ```cddl de docs/especificacao/01-*.md,
02-*.md e 03-*.md, lidos nessa ordem. O arquivo gerado:

  1. comeca com uma regra raiz, `porecatu-root`, que junta as categorias de
     vetor (host-envelope, client-envelope, handshake-start, pair-request e
     message). Quem valida um vetor usa a regra indicada no campo `raiz` dele
     (a ferramenta `rust/tools/vectors` antepoe `vector-root = <raiz>` ao
     schema); `porecatu-root` serve a quem so quer "algum item valido do
     protocolo";
  2. reproduz os blocos na ordem, cada um precedido de um comentario com o
     arquivo de origem;
  3. acrescenta `* tstr => any` ao fim de TODO mapa de corpo (todo mapa cujas
     chaves sao textos entre aspas: ver 03 §1), para o schema aceitar chave
     desconhecida como o decodificador aceita.

Sem dependencias alem da biblioteca padrao.
"""

from __future__ import annotations

import glob
import os
import re
import sys
import textwrap

# Console do Windows pode vir em codepage legado.
for fluxo in (sys.stdout, sys.stderr):
    if hasattr(fluxo, "reconfigure"):
        fluxo.reconfigure(encoding="utf-8", errors="replace")

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SPEC_DIR = os.path.join("docs", "especificacao")
SCHEMA_PATH = os.path.join("schema", "porecatu-remote.cddl")
PREFIXOS = ("01-", "02-", "03-")

CURINGA = "* tstr => any"

CABECALHO = """\
; SPDX-License-Identifier: GPL-3.0-or-later
;
; Schema CDDL do protocolo de acesso remoto do Porecatu.
;
; ARQUIVO GERADO por scripts/extract-cddl.py a partir dos blocos `cddl` de
; docs/especificacao/01-*.md, 02-*.md e 03-*.md. Nao edite: mude a
; especificacao e rode o script. O verificador de documentacao reprova o
; arquivo fora de sincronia.
;
; Todo mapa de corpo termina em `* tstr => any` (03 §1): chave desconhecida
; e ignorada. Os vetores de teste sao validados contra a regra indicada no
; campo `raiz` de cada um; `porecatu-root` e a uniao de todas as categorias.

porecatu-root = host-envelope / client-envelope / handshake-start
              / pair-request / message
"""


def arquivos_da_spec() -> list[str]:
    achados: list[str] = []
    for prefixo in PREFIXOS:
        casados = sorted(glob.glob(os.path.join(ROOT, SPEC_DIR, prefixo + "*.md")))
        achados.extend(casados)
    return achados


def blocos_cddl(texto: str) -> list[str]:
    """Blocos ```cddl do texto, sem a cerca e sem o recuo comum."""
    blocos: list[str] = []
    atual: list[str] | None = None
    for linha in texto.splitlines():
        if atual is None:
            if re.match(r"^\s*```cddl\s*$", linha):
                atual = []
        elif re.match(r"^\s*```\s*$", linha):
            blocos.append(textwrap.dedent("\n".join(atual)).strip("\n"))
            atual = None
        else:
            atual.append(linha)
    if atual is not None:
        raise ValueError("bloco cddl sem fechamento")
    return blocos


def acrescentar_curinga(bloco: str) -> tuple[str, int]:
    """Acrescenta `* tstr => any` a todo mapa de corpo do bloco.

    Mapa de corpo = `{ ... }` cuja primeira entrada tem chave de texto entre
    aspas (opcionalmente precedida de `?`). Varre o texto respeitando
    comentarios (`;` ate o fim da linha) e strings, e insere de tras para a
    frente para nao invalidar posicoes.
    """
    pilha: list[int] = []
    fechamentos: list[tuple[int, int, int]] = []  # (abre, fecha, ultimo_significativo)
    ultimo_sig = -1
    i = 0
    n = len(bloco)
    while i < n:
        c = bloco[i]
        if c == ";":
            while i < n and bloco[i] != "\n":
                i += 1
            continue
        if c == '"':
            j = i + 1
            while j < n and bloco[j] != '"':
                j += 2 if bloco[j] == "\\" else 1
            if j >= n:
                raise ValueError("string sem fechamento")
            ultimo_sig = j
            i = j + 1
            continue
        if c == "{":
            pilha.append(i)
            ultimo_sig = i
        elif c == "}":
            if not pilha:
                raise ValueError("'}' sem '{'")
            abre = pilha.pop()
            fechamentos.append((abre, i, ultimo_sig))
            ultimo_sig = i
        elif not c.isspace():
            ultimo_sig = i
        i += 1
    if pilha:
        raise ValueError("'{' sem '}'")

    insercoes: list[tuple[int, str]] = []
    for abre, fecha, sig in fechamentos:
        if not _mapa_de_corpo(bloco, abre, fecha):
            continue
        virgula = "" if bloco[sig] == "," else ","
        if "\n" in bloco[sig:fecha]:
            # O `}` esta em outra linha: o curinga ganha linha propria, com o
            # recuo da linha da ultima entrada.
            inicio_linha_fecha = bloco.rfind("\n", 0, fecha) + 1
            inicio_linha_sig = bloco.rfind("\n", 0, sig) + 1
            recuo = re.match(r"[ \t]*", bloco[inicio_linha_sig:]).group(0)
            if virgula:
                insercoes.append((sig + 1, virgula))
            insercoes.append((inicio_linha_fecha, f"{recuo}{CURINGA},\n"))
        else:
            insercoes.append((sig + 1, f"{virgula} {CURINGA}"))

    saida = bloco
    for pos, texto in sorted(insercoes, key=lambda p: p[0], reverse=True):
        saida = saida[:pos] + texto + saida[pos:]
    return saida, len([f for f in fechamentos if _mapa_de_corpo(bloco, f[0], f[1])])


def _mapa_de_corpo(bloco: str, abre: int, fecha: int) -> bool:
    corpo = bloco[abre + 1 : fecha]
    # Tira comentarios antes de olhar o primeiro token.
    sem_comentarios = re.sub(r";[^\n]*", "", corpo).lstrip()
    if sem_comentarios.startswith("?"):
        sem_comentarios = sem_comentarios[1:].lstrip()
    return sem_comentarios.startswith('"')


def gerar() -> str:
    partes = [CABECALHO]
    mapas = 0
    for caminho in arquivos_da_spec():
        with open(caminho, encoding="utf-8") as f:
            blocos = blocos_cddl(f.read())
        nome = os.path.basename(caminho)
        for bloco in blocos:
            novo, qtd = acrescentar_curinga(bloco)
            mapas += qtd
            partes.append(f"; ==== {nome} ====\n{novo}\n")
    if len(partes) == 1:
        raise ValueError("nenhum bloco cddl encontrado")
    texto = "\n".join(partes)
    if not texto.endswith("\n"):
        texto += "\n"
    return texto


def conferir() -> tuple[bool, str]:
    """Usado pelo verificador de documentacao: (em sincronia?, mensagem)."""
    caminho = os.path.join(ROOT, SCHEMA_PATH)
    try:
        esperado = gerar()
    except ValueError as e:
        return False, f"falha ao extrair o schema: {e}"
    if not os.path.exists(caminho):
        return False, f"{SCHEMA_PATH} nao existe; rode python scripts/extract-cddl.py"
    with open(caminho, encoding="utf-8", newline="") as f:
        atual = f.read().replace("\r\n", "\n")
    if atual != esperado:
        return False, (
            f"{SCHEMA_PATH} fora de sincronia com a especificacao; "
            "rode python scripts/extract-cddl.py"
        )
    return True, f"{SCHEMA_PATH} em sincronia com os blocos cddl da especificacao"


def main(argv: list[str]) -> int:
    if argv not in ([], ["--check"]):
        print("uso: extract-cddl.py [--check]", file=sys.stderr)
        return 2
    if argv == ["--check"]:
        ok, msg = conferir()
        print(msg)
        return 0 if ok else 1
    texto = gerar()
    destino = os.path.join(ROOT, SCHEMA_PATH)
    os.makedirs(os.path.dirname(destino), exist_ok=True)
    with open(destino, "w", encoding="utf-8", newline="\n") as f:
        f.write(texto)
    print(f"escrito {SCHEMA_PATH}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
