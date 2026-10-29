from pathlib import Path

p = Path('src/adapters/terminal/embedded.rs')
s = p.read_text(encoding='utf-8')
old = '''                if crate::application::update::schedule_pending_update(&paths)? {
                    return Ok(());
                }
'''
new = '''                if crate::application::update::schedule_pending_update(&paths)? {
                    // The swap helper cannot replace the running executable while this
                    // GUI process still owns it. Startup has no user session to preserve,
                    // so terminate immediately; the verified helper relaunches SST.
                    std::process::exit(0);
                }
'''
count = s.count(old)
if count != 1:
    raise SystemExit(f'updater startup exit patch: expected one match, got {count}')
p.write_text(s.replace(old, new, 1), encoding='utf-8', newline='\n')
