#!/bin/bash
S=/tmp/claude-1000/-home-thorsten-Dokumente-GitHub-freerouting/c2cb9f8a-c54e-4c50-900e-89d10f23e2ea/scratchpad
CP=$S/harness:$S/jclasses:/home/thorsten/Dokumente/GitHub/freerouting/freerouting/scripts/benchmark/binaries/freerouting-current.jar
i="${1%% *}"; p="${1#* }"; d="${p%%=*}"; s="${p#*=}"
timeout 300 java -cp $CP Dump "$d" /dev/null /dev/null "" "$s" 2>/dev/null | grep -av '^20[0-9][0-9]-[0-9][0-9]-[0-9][0-9] ' > "$S/scmp/ses$i.java.txt"
