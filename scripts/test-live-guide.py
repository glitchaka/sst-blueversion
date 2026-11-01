#!/usr/bin/env python3
import json
import re
import sys
import urllib.request
from collections import defaultdict

BASE = "https://raw.githubusercontent.com/glitchaka/adm-toolbox/main/tutorials"
MAX_INDEX = 2 * 1024 * 1024
MAX_MANUAL = 512 * 1024


def fetch(name: str) -> bytes:
    req = urllib.request.Request(f"{BASE}/{name}", headers={"User-Agent": "SST-QA/1"})
    with urllib.request.urlopen(req, timeout=30) as response:
        data = response.read(MAX_INDEX + 1 if name == "guide-index.json" else MAX_MANUAL + 1)
    return data


def is_numbered_heading(line: str) -> bool:
    line = line.strip()
    if "." not in line:
        return False
    number, rest = line.split(".", 1)
    return bool(number) and number.isdigit() and rest.startswith(" ")


def heading_tokens(line: str):
    rest = line.split(".", 1)[1] if "." in line else line
    parts = re.split(r"[^\w:]+", rest, flags=re.UNICODE)
    return [p.strip(":").lower() for p in parts if p.strip(":")]


def extract_numbered(raw: str, keyword: str):
    lines = raw.splitlines()
    wanted = [part.lower() for part in keyword.split()]
    start = None
    for i, line in enumerate(lines):
        if not is_numbered_heading(line):
            continue
        tokens = heading_tokens(line)
        if all(part in tokens for part in wanted):
            start = i
            break
    if start is None:
        return None
    end = len(lines)
    for i in range(start + 1, len(lines)):
        if is_numbered_heading(lines[i]):
            end = i
            break
    return "\n".join(lines[start:end]).strip()


def main():
    raw = fetch("guide-index.json")
    size = len(raw)
    if size > MAX_INDEX:
        raise AssertionError(f"guide-index.json excede 2 MiB: {size}")
    headroom = MAX_INDEX - size
    print(f"guide-index.json: {size} bytes; margen={headroom} bytes")
    if headroom < 128 * 1024:
        print(f"WARNING: margen de índice bajo ({headroom} bytes)")

    guide = json.loads(raw.decode("utf-8"))
    assert guide.get("schema") == 1, "schema de guide inválido"
    areas = guide.get("areas")
    entries = guide.get("entries")
    assert isinstance(areas, list) and areas, "areas vacío"
    assert isinstance(entries, list), "entries inválido"

    area_ids = [a.get("id", "").strip().lower() for a in areas]
    assert all(area_ids), "área sin id"
    assert len(area_ids) == len(set(area_ids)), "ids de área duplicados"
    assert "arch" in area_ids, "falta Arch / Portabilidad"
    assert area_ids[-1] == "arch", "Arch debe estar al final, fuera del bloque normal"
    area_set = set(area_ids)

    names = set()
    lookup = {}
    arch_entries = []
    refs = defaultdict(list)
    for entry in entries:
        name = str(entry.get("name", "")).strip()
        key = name.lower()
        assert name, "entrada sin nombre"
        assert key not in names, f"entrada duplicada: {name}"
        if key in lookup:
            raise AssertionError(f"nombre {name!r} sobrescribiría alias previo de {lookup[key]!r}")
        names.add(key)
        lookup[key] = name
        for alias in entry.get("aliases", []):
            alias_key = str(alias).strip().lower()
            if alias_key and alias_key not in lookup:
                lookup[alias_key] = name

        area = str(entry.get("area", "")).strip().lower()
        assert area in area_set, f"{name}: área inexistente {area}"
        manual = entry.get("manual")
        assert isinstance(manual, dict), f"{name}: manual inválido"
        if area == "arch":
            arch_entries.append(entry)
            file = manual.get("file", "")
            assert re.fullmatch(r"arch-(?:[0-9a-f]{2}|p-[0-9a-f]{12})", file), (
                f"{name}: shard inválido {file}"
            )
            assert manual.get("section") == "unix", f"{name}: section debe ser unix"
            topic = str(manual.get("topic", "")).strip()
            assert topic, f"{name}: topic vacío"
            refs[file].append(entry)

    assert len(arch_entries) >= 15000, f"catálogo Arch incompleto: {len(arch_entries)}"
    assert len(refs) >= 240, f"muy pocos shards Arch: {len(refs)}"
    print(f"Arch entries: {len(arch_entries)}; manuals: {len(refs)}")

    failures = []
    checked = 0
    for shard in sorted(refs):
        filename = f"{shard}.txt"
        data = fetch(filename)
        if len(data) > MAX_MANUAL:
            failures.append(f"{filename}: excede 512 KiB ({len(data)})")
            continue
        try:
            text = data.decode("utf-8")
        except UnicodeDecodeError as exc:
            failures.append(f"{filename}: UTF-8 inválido: {exc}")
            continue
        for entry in refs[shard]:
            display_name = entry["name"]
            package_name = entry.get("package", display_name)
            topic = entry["manual"]["topic"]
            section = extract_numbered(text, topic)
            checked += 1
            if section is None:
                failures.append(
                    f"{display_name}: sección no encontrada en {filename} para topic={topic!r}"
                )
                continue
            expected = f"Paquete: {package_name}"
            if expected not in section.splitlines():
                first = section.splitlines()[0] if section else "<vacío>"
                failures.append(
                    f"{display_name}: resolución ambigua/incorrecta en {filename}; obtuvo {first!r}"
                )

    if failures:
        print("\nFAILURES:")
        for item in failures[:100]:
            print(" -", item)
        if len(failures) > 100:
            print(f" ... y {len(failures) - 100} más")
        raise AssertionError(f"fallaron {len(failures)} resoluciones Arch de {checked}")

    try:
        json.loads(b"{broken")
        raise AssertionError("JSON corrupto fue aceptado")
    except json.JSONDecodeError:
        pass

    print(f"PASS: {checked} fichas Arch resuelven exactamente a su paquete")
    print("PASS: estructura, lookup, duplicados, límites y referencias del guide válidos")


if __name__ == "__main__":
    try:
        main()
    except Exception as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        raise
