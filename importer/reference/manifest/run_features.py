"""Run the pinned Cargo feature constructor through offline empty workspaces."""
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile

PIN = '59800466c5c41c444d264b1010b4d57e85a7117f'
TOOLCHAIN = 'nightly-2026-07-11'
SOURCE = Path('/private/tmp/rawr-p9-pinned-cargo-full') / ('cargo-' + PIN) / 'src/cargo/core/features.rs'
assert hashlib.sha256(SOURCE.read_bytes()).hexdigest() == '43b9479784a684a828876fddf34a3f5e881bcf743c493cd3ae17074c644270d4'
source = SOURCE.read_text()
names = [(name.replace('_', '-'), status) for status, name in re.findall(r'^    \((stable|unstable|removed), (\w+),', source, re.M)]
assert len(names) == 27
cli_source = source.split('unstable_cli_options!(', 1)[1].split('\n);', 1)[0]
cli_names = re.findall(r'^    (\w+):', cli_source, re.M)
assert len(cli_names) == 54, len(cli_names)
binary = Path(subprocess.check_output(['rustup', 'which', '--toolchain', TOOLCHAIN, 'cargo'], text=True).strip())
version = subprocess.check_output([str(binary), '--version', '--verbose'], text=True)
assert 'commit-hash: ' + PIN in version

cases = []
def add(name, features, channel='nightly', allow=None):
    cases.append(dict(name=name, features=features, channel=channel, allow=allow))
for name, status in names:
    for channel in ['nightly', 'dev', 'stable', 'beta', 'custom', '']:
        add(name + ':' + channel, [name], channel)
    for suffix, allow in [('empty', []), ('exact', [name]), ('wrong', ['different'])]:
        add(name + ':allow-' + suffix, [name], allow=allow)
    add(name + ':duplicate', [name, name])
for name in cli_names:
    name = name.replace('_', '-')
    if name not in dict(names):
        add('cli:' + name, [name])
for name in ['unknown', 'profile_rustflags', 'Profile-rustflags', '', ' edition', 'edition ', 'é', 'é_d', 'profile--rustflags']:
    add('unknown:' + name, [name])
for features in [['edition', 'unknown'], ['unknown', 'edition'], ['edition', 'edition', 'unknown'], ['publish-lockfile', 'unknown'], ['unknown', 'publish-lockfile'], ['edition', 'profile-rustflags']]:
    add('order:' + ','.join(features), features, allow=[])
add('all-successful', [name for name, status in names if status != 'removed'])
add('absent', [])

def classify(stderr):
    if 'unknown Cargo.toml feature' in stderr:
        if "Feature names must use '-' instead of '_'." in stderr:
            return 'UnknownUnderscore'
        if 'This feature can be enabled via -Z' in stderr:
            return 'UnknownCli'
        return 'Unknown'
    for text, kind in [
        ('has already been activated', 'Duplicate'),
        ('requires a nightly version of Cargo', 'Channel'),
        ('is not in the list of allowed features', 'Denied'),
        ('has been removed in the', 'Removed'),
    ]:
        if text in stderr:
            return kind
    raise AssertionError(stderr)

with tempfile.TemporaryDirectory(prefix='rawr-p9-feature-corpus-') as temporary:
    root = Path(temporary)
    home = root / 'cargo-home'
    home.mkdir()
    rows = []
    for index, case in enumerate(cases):
        fixture = root / str(index)
        fixture.mkdir()
        manifest = 'cargo-features=' + json.dumps(case['features'], ensure_ascii=False) + '\n[workspace]\n'
        (fixture / 'Cargo.toml').write_text(manifest)
        env = {key: value for key, value in os.environ.items() if not key.startswith('CARGO_') and key not in ['RUSTC_BOOTSTRAP', '__CARGO_TEST_CHANNEL_OVERRIDE_DO_NOT_USE_THIS']}
        env.update(CARGO_HOME=str(home), __CARGO_TEST_CHANNEL_OVERRIDE_DO_NOT_USE_THIS=case['channel'])
        command = [str(binary), 'metadata', '--format-version=1', '--no-deps', '--offline']
        if case['allow'] is not None:
            command += ['--config', 'unstable.allow-features=' + json.dumps(case['allow'])]
        result = subprocess.run(command, cwd=fixture, env=env, capture_output=True, text=True)
        rows.append(dict(case, source=manifest, returncode=result.returncode, tag=classify(result.stderr) if result.returncode else None, stderr=result.stderr.replace(str(root), '<fixture-root>')))

base = Path(__file__).resolve().parent
output = base / 'feature-observations.jsonl'
output.write_text(''.join(json.dumps(row, ensure_ascii=False) + '\n' for row in rows))
summary = dict(cargo_commit=PIN, toolchain=TOOLCHAIN, binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(), cases=len(rows), accepted=sum(row['tag'] is None for row in rows), files={path.name: hashlib.sha256(path.read_bytes()).hexdigest() for path in [Path(__file__), output]}, command=['<pinned-cargo>', 'metadata', '--format-version=1', '--no-deps', '--offline', '[--config unstable.allow-features=<list>]'], scope='Constructor acceptance and first rejection via actual Cargo; metadata suppresses warnings. Context uses isolated Cargo home/cwd and explicit channel override. No require/query or nonlocal source execution claim.')
(base / 'feature-execution.json').write_text(json.dumps(summary, indent=2) + '\n')
print(json.dumps(summary))
