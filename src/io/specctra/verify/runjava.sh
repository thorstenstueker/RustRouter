#!/bin/bash
S=/tmp/claude-1000/-home-thorsten-Dokumente-GitHub-freerouting/c2cb9f8a-c54e-4c50-900e-89d10f23e2ea/scratchpad
CP=$S/harness:$S/jclasses:/home/thorsten/Dokumente/GitHub/freerouting/freerouting/scripts/benchmark/binaries/freerouting-current.jar
f="$1"; n=$(echo "$f" | tr '/' '_')
timeout 300 java -cp $CP Dump "$f" "$S/cmp/$n.java.ses" "$S/cmp/$n.java.rules" > "$S/cmp/$n.java.txt" 2> "$S/cmp/$n.java.err"
