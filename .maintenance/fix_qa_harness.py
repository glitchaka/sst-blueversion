from pathlib import Path

p = Path('scripts/terminal_blackbox_stress.py')
s = p.read_text(encoding='utf-8')
replacements = [
    (
        'h.contains("embedded-pty-transport", ["--verify-terminal"], "SST_NATIVE_OK", 0, "transport", 20)',
        'h.contains("embedded-pty-transport", ["--verify-terminal"], "Shell Shock Tool: prompt, entrada, ejecución y salida correctos.", 0, "transport", 20)',
    ),
    (
        '("awk", \'printf "a b\\\\n" | awk "{print $2}"\', "b\\n"),',
        '("awk", "printf \'a b\\\\n\' | awk \'{print $2}\'", "b\\n"),',
    ),
]
for old, new in replacements:
    count = s.count(old)
    if count != 1:
        raise SystemExit(f'QA harness replacement expected once, got {count}: {old}')
    s = s.replace(old, new, 1)
p.write_text(s, encoding='utf-8', newline='\n')
