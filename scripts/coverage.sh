#!/bin/bash
#
# Code coverage script for otsim-simulator
#
# Usage:
#   ./scripts/coverage.sh              # Generate HTML report
#   ./scripts/coverage.sh --lcov       # Generate LCOV for CI
#   ./scripts/coverage.sh --summary    # Show summary only
#   ./scripts/coverage.sh --threshold  # Check against threshold

set -e

# Configuration
COVERAGE_DIR="target/coverage"
THRESHOLD=${COVERAGE_THRESHOLD:-80}
REPORT_TYPE="html"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --lcov)
            REPORT_TYPE="lcov"
            shift
            ;;
        --summary)
            REPORT_TYPE="summary"
            shift
            ;;
        --threshold)
            REPORT_TYPE="threshold"
            shift
            ;;
        --threshold=*)
            THRESHOLD="${1#*=}"
            REPORT_TYPE="threshold"
            shift
            ;;
        -h|--help)
            echo "Usage: $0 [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  --lcov        Generate LCOV report for CI"
            echo "  --summary     Show coverage summary"
            echo "  --threshold   Check coverage against threshold (default: 80%)"
            echo "  --threshold=N Set custom threshold"
            echo "  -h, --help    Show this help"
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

# Check if cargo-llvm-cov is installed
if ! command -v cargo-llvm-cov &> /dev/null; then
    echo -e "${YELLOW}Installing cargo-llvm-cov...${NC}"
    cargo install cargo-llvm-cov
fi

# Create coverage directory
mkdir -p "$COVERAGE_DIR"

echo -e "${BLUE}========================================${NC}"
echo -e "${BLUE}  TRAP Simulator Code Coverage${NC}"
echo -e "${BLUE}========================================${NC}"
echo ""

case $REPORT_TYPE in
    html)
        echo -e "${YELLOW}Generating HTML coverage report...${NC}"
        cargo llvm-cov --workspace --all-features \
            --html \
            --output-dir "$COVERAGE_DIR/html" \
            2>&1 | tee "$COVERAGE_DIR/coverage.log"

        echo ""
        echo -e "${GREEN}HTML report generated: $COVERAGE_DIR/html/index.html${NC}"

        # Try to open in browser
        if command -v open &> /dev/null; then
            open "$COVERAGE_DIR/html/index.html"
        elif command -v xdg-open &> /dev/null; then
            xdg-open "$COVERAGE_DIR/html/index.html"
        fi
        ;;

    lcov)
        echo -e "${YELLOW}Generating LCOV report...${NC}"
        cargo llvm-cov --workspace --all-features \
            --lcov \
            --output-path "$COVERAGE_DIR/lcov.info" \
            2>&1 | tee "$COVERAGE_DIR/coverage.log"

        echo ""
        echo -e "${GREEN}LCOV report generated: $COVERAGE_DIR/lcov.info${NC}"
        ;;

    summary)
        echo -e "${YELLOW}Coverage Summary:${NC}"
        echo ""
        cargo llvm-cov --workspace --all-features 2>&1 | tee "$COVERAGE_DIR/summary.txt"
        ;;

    threshold)
        echo -e "${YELLOW}Checking coverage against threshold: ${THRESHOLD}%${NC}"
        echo ""

        # Get coverage percentage
        OUTPUT=$(cargo llvm-cov --workspace --all-features 2>&1)
        echo "$OUTPUT"

        # Extract coverage percentage (look for "total" line)
        COVERAGE=$(echo "$OUTPUT" | grep -E "^\s*TOTAL" | awk '{print $NF}' | tr -d '%')

        if [ -z "$COVERAGE" ]; then
            # Try alternative format
            COVERAGE=$(echo "$OUTPUT" | grep -E "coverage:" | tail -1 | grep -oE '[0-9]+\.[0-9]+' | head -1)
        fi

        if [ -z "$COVERAGE" ]; then
            echo -e "${RED}Could not parse coverage percentage${NC}"
            exit 1
        fi

        echo ""
        echo "=========================================="
        echo -e "Coverage: ${COVERAGE}%"
        echo -e "Threshold: ${THRESHOLD}%"
        echo "=========================================="

        # Compare (using bc for floating point)
        if command -v bc &> /dev/null; then
            PASS=$(echo "$COVERAGE >= $THRESHOLD" | bc -l)
        else
            # Fallback to integer comparison
            COVERAGE_INT=${COVERAGE%.*}
            PASS=$((COVERAGE_INT >= THRESHOLD ? 1 : 0))
        fi

        if [ "$PASS" -eq 1 ]; then
            echo -e "${GREEN}✓ Coverage meets threshold${NC}"
            exit 0
        else
            echo -e "${RED}✗ Coverage below threshold${NC}"
            exit 1
        fi
        ;;
esac

echo ""
echo -e "${GREEN}Done!${NC}"
