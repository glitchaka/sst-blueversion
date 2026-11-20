#!/usr/bin/env python3
"""Create an SST-PACKAGE/1 .pkgx (gzip-compressed TAR, never an EXE).
Archive layout: manifest.json, payload/{win|unix}/{modules|runtimes|environments|data}/{id}/...
"""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import sys
import tarfile

GROUPS = {
    "embedded": "modules",
    "runtime": "runtimes",
    "environment": "environments",
    "data": "data",
}
BAD_SUFFIXES = {".exe", ".com", ".bat", ".cmd"}


def main() -> None:
    parser = argparse.ArgumentParser(description="Construir paquete nativo SST sin ejecutables externos")
    parser.add_argument("--id", required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--target", choices=["win", "unix"], required=True)
    parser.add_argument("--kind", choices=list(GROUPS), required=True)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--exclude-dotnet-apphost", action="store_true")
    parser.add_argument("--require-file", action="append", default=[], help="Archivo obligatorio en payload; repetible")
    args = parser.parse_args()
    if not args.id or args.id in (".", "..") or not all(c.isascii() and (c.isalnum() or c in "-_.") for c in args.id):
        parser.error("id inválido")
    if args.output.suffix.lower() != ".pkgx":
        parser.error("el archivo de salida debe terminar en .pkgx")
    if not args.source.is_dir():
        parser.error("el directorio de origen no existe")
    prefix = f"payload/{args.target}/{GROUPS[args.kind]}/{args.id}/"
    members = []
    for file in sorted(args.source.rglob("*")):
        if file.is_symlink():
            parser.error(f"no se admiten enlaces simbólicos: {file}")
        if file.is_dir():
            continue
        if not file.is_file():
            parser.error(f"archivo no regular: {file}")
        relative = file.relative_to(args.source).as_posix()
        if file.suffix.lower() in BAD_SUFFIXES:
            if args.exclude_dotnet_apphost and relative in (
                "Sst.PowerShell.Bridge.exe", "createdump.exe"
            ):
                # The self-contained runtime publishes an optional diagnostics
                # executable and an apphost. SST loads only the DLL via hostfxr.
                print(f"Ejecutable auxiliar .NET excluido: {relative}")
                continue
            parser.error(f"un paquete SST no puede contener ejecutables externos: {relative}")
        members.append((prefix + relative, file))
    if not members:
        parser.error("paquete vacío")
    present = {file.relative_to(args.source).as_posix() for _, file in members}
    missing = sorted(set(args.require_file) - present)
    if missing:
        parser.error(f"paquete incompleto: faltan archivos obligatorios {missing}")
    manifest = {
        "format": 1,
        "id": args.id,
        "version": args.version,
        "kind": args.kind,
        "files": [
            {"path": name, "sha256": hashlib.sha256(file.read_bytes()).hexdigest()}
            for name, file in members
        ],
    }
    body = (json.dumps(manifest, ensure_ascii=False, indent=2) + "\n").encode("utf-8")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with tarfile.open(args.output, "w:gz", format=tarfile.PAX_FORMAT) as tar:
        header = tarfile.TarInfo("manifest.json")
        header.size = len(body)
        header.mode = 0o644
        header.mtime = 0
        tar.addfile(header, io.BytesIO(body))
        for name, file in members:
            header = tar.gettarinfo(str(file), arcname=name)
            header.uid = header.gid = 0
            header.uname = header.gname = ""
            header.mtime = 0
            header.mode = 0o644
            with file.open("rb") as payload:
                tar.addfile(header, payload)
    print(f"Creado {args.output}: {len(members)} archivos, {args.target}/{GROUPS[args.kind]}/{args.id}")


if __name__ == "__main__":
    main()
