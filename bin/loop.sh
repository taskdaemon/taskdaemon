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
        echo -e "${RED}Max iterations reached. Exiting.${NC}"
        exit 1
    fi

    # Run Claude interactively (no -p flag - shows full TUI output)
    claude --model "$MODEL" \
        --dangerously-skip-permissions \
        < "$PROMPT_FILE"

    # Check for completion marker + CI validation
    echo ""
    if [[ -f ".taskdaemon-complete" ]]; then
        echo -e "${YELLOW}Completion marker found. Running final validation (otto ci)...${NC}"
        if otto ci; then
            echo -e "${GREEN}=== ALL PHASES COMPLETE ===${NC}"
            echo -e "${GREEN}Loop finished after $iteration iterations.${NC}"
            cat .taskdaemon-complete
            exit 0
        else
            echo -e "${RED}Completion marker exists but CI failed. Removing marker...${NC}"
            rm -f .taskdaemon-complete
        fi
    else
        echo -e "${YELLOW}No completion marker yet. Running CI check...${NC}"
        if otto ci; then
            echo -e "${YELLOW}CI passes but .taskdaemon-complete not found.${NC}"
            echo -e "${YELLOW}More phases to implement. Continuing...${NC}"
        else
            echo -e "${RED}CI failed. Continuing...${NC}"
        fi
    fi

    # Optional: push changes after each iteration
    # git push origin $(git branch --show-current) 2>/dev/null || true

    echo ""
    echo -e "${YELLOW}Sleeping ${SLEEP_BETWEEN}s before next iteration...${NC}"
    sleep "$SLEEP_BETWEEN"
done
