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
    ((iteration++))
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
    ((iteration++))
