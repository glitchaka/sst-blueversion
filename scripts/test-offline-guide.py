#!/usr/bin/env python3
"""SST: valida sin red la instantánea local y sus referencias de guide."""
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
GUIDE = ROOT / "guide"
INDEX = GUIDE / "guide-index.json"
MAX_INDEX = 2 * 1024 * 1024
MAX_MANUAL = 512 * 1024


def number_heading(line):
    return bool(re.match(r"^\s*\d+\.\s", line))


def extract_numbered(text, keyword):
    parts = [word.casefold() for word in keyword.split()]
    lines = text.splitlines()
    for idx, line in enumerate(lines):
        if not number_heading(line):
            continue
        suffix = line.split(".", 1)[1]
        tokens = [token.strip(":").casefold() for token in re.split(r"[^\w:]+", suffix) if token]
        if all(part in tokens for part in parts):
            stop = next((next_idx for next_idx in range(idx + 1, len(lines))
                         if number_heading(lines[next_idx])), len(lines))
            return "\n".join(lines[idx:stop]).strip()
    return None


def resolve(text, section, topic):
    if section == "whole":
        return text.strip()
    if section == "unix":
        return extract_numbered(text, "Básicos de UnixService" if topic in ("env", "clear") else topic)
    if section == "bash":
        if topic in (":", "true", "false"):
            return extract_numbered(text, "Builtins mínimos")
        lines = text.splitlines()
        floor = next((i for i, line in enumerate(lines) if line.lstrip().startswith("2.")), 0)
        for i in range(floor, len(lines)):
            if lines[i] == lines[i].strip() and lines[i].casefold() == topic.casefold():
                return lines[i].strip()
        exact = extract_numbered(text, topic)
        if exact:
            return exact
        sections = [i for i, line in enumerate(lines) if number_heading(line)]
        for position, start in enumerate(sections[1:], 1):
            stop = sections[position + 1] if position + 1 < len(sections) else len(lines)
            for line in lines[start + 1:stop]:
                stripped = line.lstrip()
                if stripped.startswith(topic) and (
                    len(stripped) == len(topic)
                    or stripped[len(topic)].isspace()
                    or stripped[len(topic)] in "[:/(="
                ):
                    return "\n".join(lines[start:stop]).strip()
        return extract_numbered(text, "Alcance")
    return None


def main():
    raw = INDEX.read_bytes()
    assert 0 < len(raw) <= MAX_INDEX, "El índice local no cumple límites"
    index = json.loads(raw.decode("utf-8"))
    assert index.get("schema") == 1
    areas = index["areas"]
    entries = index["entries"]
    area_ids = [area["id"] for area in areas]
    assert len(area_ids) == len(set(area_ids))
    assert "arch" not in area_ids, "Arch no debe aparecer en el menú"
    assert all(e["area"] != "arch" for e in entries), "Arch no debe aparecer en búsquedas"
    assert len(entries) >= 100, "Faltan fichas locales de SST"

    names = set()
    manuals = set()
    checked = 0
    for entry in entries:
        name = entry["name"].strip()
        assert name and name.lower() not in names, f"Entrada duplicada: {name}"
        names.add(name.lower())
        assert entry["area"] in area_ids, f"{name}: área desconocida"
        manual = entry["manual"]
        filename = manual["file"] + "." + manual.get("extension", "txt")
        assert re.fullmatch(r"[A-Za-z0-9_-]+\.(?:txt|md)", filename), f"{name}: ruta no permitida"
        manual_path = GUIDE / filename
        assert manual_path.is_file(), f"{name}: falta archivo local {filename}"
        size = manual_path.stat().st_size
        assert 0 < size <= MAX_MANUAL, f"{filename}: tamaño incorrecto {size}"
        text = manual_path.read_text(encoding="utf-8")
        section = manual.get("section", "whole")
        topic = manual.get("topic") or ""
        assert resolve(text, section, topic), f"{name}: no se pudo resolver {filename} [{section}:{topic}]"
        manuals.add(filename)
        checked += 1

    assert len(manuals) >= 30, "Faltan manuales referenciados"
    print(f"PASS: Guide SST local, {len(areas)} áreas, {checked} fichas, {len(manuals)} archivos")
    print("PASS: Arch oculto, rutas válidas, cada ficha se resuelve sin Internet")


if __name__ == "__main__":
    main()
