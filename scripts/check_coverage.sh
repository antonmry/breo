#!/bin/bash
# Check per-file line coverage via cargo llvm-cov.
# Fails if any source file is below the threshold.
#
# Note: Functions that spawn external processes (LLM backends, Discord API,
# git operations) are not unit-testable without mocking. The threshold
# reflects what's achievable with pure-logic and filesystem tests.

THRESHOLD=70

cargo llvm-cov --json 2>/dev/null | python3 -c "
import json, sys

d = json.load(sys.stdin)
failed = False
for f in d['data'][0]['files']:
    s = f['summary']['lines']
    pct = s['percent'] if s['count'] > 0 else 0.0
    name = f['filename']
    if pct < $THRESHOLD:
        print(f'FAIL  {pct:5.1f}%  {name}')
        failed = True
    else:
        print(f'  OK  {pct:5.1f}%  {name}')

if failed:
    print()
    print('Coverage below ${THRESHOLD}% threshold.')
    sys.exit(1)
else:
    total = d['data'][0]['totals']['lines']
    print(f'\nOverall: {total[\"percent\"]:.1f}%')
"
