#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-only
# Morpheus-Hybrid Unsafe Audit Script
#
# Lists all `unsafe` blocks and verifies they are documented with reasons.

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

echo "=== Morpheus-Hybrid Unsafe Audit ==="
echo ""

# Find all unsafe blocks in Rust files
echo "Scanning for unsafe blocks..."
echo ""

UNSAFE_COUNT=0
UNDOCUMENTED=0

# Search for unsafe blocks
while IFS= read -r result; do
    FILE=$(echo "$result" | cut -d: -f1)
    LINE=$(echo "$result" | cut -d: -f2)
    
    ((UNSAFE_COUNT++))
    
    # Check if the previous line contains a safety comment
    PREV_LINE=$((LINE - 1))
    PREV_CONTENT=$(sed -n "${PREV_LINE}p" "$FILE" 2>/dev/null || echo "")
    
    if echo "$PREV_CONTENT" | grep -qi "safety\|morpheus_unsafe\|# unsafe\|// unsafe"; then
        STATUS="✅ Documented"
    else
        STATUS="⚠️  UNDOCUMENTED"
        ((UNDOCUMENTED++))
    fi
    
    echo "  $STATUS: $FILE:$LINE"
    
done < <(grep -rn "unsafe {" "$PROJECT_ROOT/morpheus-common/src" "$PROJECT_ROOT/morpheus-runtime/src" 2>/dev/null || true)
done < <(grep -rn "unsafe fn" "$PROJECT_ROOT/morpheus-common/src" "$PROJECT_ROOT/morpheus-runtime/src" 2>/dev/null || true)
done < <(grep -rn "unsafe impl" "$PROJECT_ROOT/morpheus-common/src" "$PROJECT_ROOT/morpheus-runtime/src" 2>/dev/null || true)

echo ""
echo "=== Summary ==="
echo "Total unsafe occurrences: $UNSAFE_COUNT"
echo "Undocumented: $UNDOCUMENTED"

if [ "$UNDOCUMENTED" -gt 0 ]; then
    echo ""
    echo "❌ Some unsafe blocks are undocumented!"
    echo "   Please add safety comments before each unsafe block."
    echo "   Example: // SAFETY: reason why this is safe"
    exit 1
else
    echo ""
    echo "✅ All unsafe blocks are documented!"
    exit 0
fi
