#!/usr/bin/env bash
# Ralph Wiggum Loop for Claude Code
# Based on Geoffrey Huntley's original technique
# https://ghuntley.com/ralph/
set -euo pipefail

# Configuration (override via environment variables)
MAX_ITERATIONS=${MAX_ITERATIONS:-50}
PROMPT_FILE=${PROMPT_FILE:-PROMPT.md}
MODEL=${MODEL:-opus}
SLEEP_BETWEEN=${SLEEP_BETWEEN:-2}

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Get script directory and project root
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

cd "$PROJECT_ROOT"

echo -e "${GREEN}=== Ralph Wiggum Loop ===${NC}"
echo "Project: $PROJECT_ROOT"
echo "Prompt:  $PROMPT_FILE"
echo "Model:   $MODEL"
echo "Max:     $MAX_ITERATIONS iterations"
echo ""

# Check prompt file exists
if [[ ! -f "$PROMPT_FILE" ]]; then
    echo -e "${RED}Error: $PROMPT_FILE not found in project root${NC}"
    echo "Create a PROMPT.md file with instructions for Claude."
    exit 1
fi

iteration=0

while true; do
    ((++iteration))

    echo -e "${YELLOW}=== Iteration $iteration of $MAX_ITERATIONS ===${NC}"

    if [[ $iteration -gt $MAX_ITERATIONS ]]; then
        echo -e "${GREEN}Max iterations reached. Exiting.${NC}"
        exit 0
    fi

    # Run Claude with the prompt
    cat "$PROMPT_FILE" | claude -p \
        --dangerously-skip-permissions \
        --model "$MODEL" \
        --verbose

    # Optional: push changes after each iteration
    # git push origin $(git branch --show-current) 2>/dev/null || true

    echo ""
    echo -e "${GREEN}Iteration $iteration complete. Sleeping ${SLEEP_BETWEEN}s...${NC}"
    sleep "$SLEEP_BETWEEN"
done
