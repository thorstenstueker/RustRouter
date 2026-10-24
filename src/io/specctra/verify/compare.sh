#!/bin/bash
cd /tmp/claude-1000/-home-thorsten-Dokumente-GitHub-freerouting/c2cb9f8a-c54e-4c50-900e-89d10f23e2ea/scratchpad/cmp
same=0; d=0; sesd=0
for f in *.java.txt; do n=${f%.java.txt}
  grep -av '^20[0-9][0-9]-[0-9][0-9]-[0-9][0-9] ' "$f" | grep -av '^\s*at \|^Exception\|^java\.' > "$n.java.clean"
  if cmp -s "$n.java.clean" "$n.rust.txt"; then same=$((same+1)); else d=$((d+1)); echo "DIFF $n $(diff "$n.java.clean" "$n.rust.txt" | grep -c '^[<>]')"; fi
  if [ -f "$n.java.ses" ] || [ -f "$n.rust.ses" ]; then if ! cmp -s "$n.java.ses" "$n.rust.ses"; then sesd=$((sesd+1)); echo "SESDIFF $n"; fi; fi
done
echo "same=$same diff=$d sesdiff=$sesd"
