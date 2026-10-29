from pathlib import Path

p = Path('.maintenance/apply_terminal_fixes.py')
s = p.read_text(encoding='utf-8')
old = '''s = replace_once(
    s,
    \'\'\'            if !spec.quoted {
                body = self.expand_scalar(&body)?;
            }
\'\'\',
    \'\'\'            if !spec.quoted {
                body = format!("\\\\u{1f}SST_HEREDOC_EXPAND\\\\u{1f}{body}");
            }
\'\'\',
    "defer heredoc expansion",
)
'''
new = '''heredoc_pattern = re.compile(
    r'if\\s+!spec\\.quoted\\s*\\{\\s*body\\s*=\\s*self\\.expand_scalar\\(&body\\)\\?;\\s*\\}',
    re.S,
)
heredoc_replacement = 'if !spec.quoted {\\n                body = format!("\\\\u{1f}SST_HEREDOC_EXPAND\\\\u{1f}{body}");\\n            }'
s, heredoc_count = heredoc_pattern.subn(lambda _match: heredoc_replacement, s, count=1)
if heredoc_count != 1:
    raise SystemExit(f"defer heredoc expansion: expected 1 match, got {heredoc_count}")
'''
if old not in s:
    raise SystemExit('old heredoc patch block not found in maintenance script')
p.write_text(s.replace(old, new, 1), encoding='utf-8', newline='\n')
