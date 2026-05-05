#!/bin/bash
set -euo pipefail

errors=0
INDEX=".agents/index.yaml"

if [ ! -f "$INDEX" ]; then
  echo "ERROR: $INDEX not found"
  exit 1
fi

echo "=== Checking .agents/ control plane integrity ==="

# --- Check read_order and read_on_demand files exist ---
for section in read_order read_on_demand; do
  while IFS= read -r line; do
    file="${line#  - }"
    if [ -n "$file" ]; then
      if [ ! -f "$file" ]; then
        echo "ERROR: $section file missing: $file"
        errors=$((errors+1))
      else
        echo "  OK: $section file exists: $file"
      fi
    fi
  done < <(sed -n "/^$section:/,/^[a-z]/p" "$INDEX" | grep '^  - ')
done

# --- Check entrypoints reference existing runbooks and tasks ---
while IFS= read -r line; do
  key="${line%%:*}"
  ref="${line#*: }"
  ref="${ref#"${ref%%[![:space:]]*}"}"

  if [[ $ref == runbook.* ]]; then
    name="${ref#runbook.}"
    f=".agents/runbooks/${name}.yaml"
    if [ ! -f "$f" ]; then
      echo "ERROR: entrypoint '$key' references missing runbook: $f"
      errors=$((errors+1))
    else
      echo "  OK: runbook exists: $f"
    fi
  elif [[ $ref == task.* ]]; then
    f=".agents/tasks/queued/${ref}.yaml"
    if [ ! -f "$f" ]; then
      echo "ERROR: entrypoint '$key' references missing task: $f"
      errors=$((errors+1))
    else
      echo "  OK: task exists: $f"
    fi
  fi
done < <(grep -E '_(runbook|task):' "$INDEX")

# --- Check recommended_primitives ---
while IFS= read -r line; do
  prim="${line#  - primitive.}"
  f=".agents/primitives/${prim}.yaml"
  if [ ! -f "$f" ]; then
    echo "ERROR: recommended primitive missing: $f"
    errors=$((errors+1))
  else
    echo "  OK: recommended primitive exists: $f"
  fi
done < <(grep -E '^  - primitive\.' "$INDEX")

# --- Check recommended_runbooks ---
while IFS= read -r line; do
  rbook="${line#  - runbook.}"
  f=".agents/runbooks/${rbook}.yaml"
  if [ ! -f "$f" ]; then
    echo "ERROR: recommended runbook missing: $f"
    errors=$((errors+1))
  else
    echo "  OK: recommended runbook exists: $f"
  fi
done < <(grep -E '^  - runbook\.' "$INDEX")

# --- Validate YAML syntax for clean directories ---
if command -v python3 &> /dev/null && python3 -c "import yaml" &> /dev/null 2>&1; then
  echo "=== Validating YAML syntax (schemas, primitives, runbooks) ==="
  while IFS= read -r f; do
    if python3 -c "
import yaml, sys
try:
    with open('$f') as fh:
        yaml.safe_load(fh)
except yaml.YAMLError as e:
    print(f'YAML error: {e}')
    sys.exit(1)
" 2>/dev/null; then
      echo "  OK: valid YAML: $f"
    else
      echo "ERROR: YAML syntax error in $f"
      errors=$((errors+1))
    fi
  done < <(find .agents/schemas .agents/primitives .agents/runbooks -name '*.yaml' -type f 2>/dev/null | sort)
else
  echo "  SKIP: python3-yaml not available for YAML validation"
fi

# --- Report ---
if [ "$errors" -gt 0 ]; then
  echo ""
  echo "FAILED: $errors integrity error(s) found"
  exit 1
fi

echo ""
echo "OK: all .agents/ integrity checks passed"
