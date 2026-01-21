# TaskDaemon Phases Index

This directory contains implementation phases (atomic implementation units) produced from specs.

## Phase Overview

| ID | Name | Spec | Dependencies | Status |
|----|------|------|--------------|--------|
| 001-pipeline-model | [Pipeline Model](./001-pipeline-model.md) | 019-pipeline-config | None | Ready |
| 002-pipeline-execution-engine | [Pipeline Execution Engine](./002-pipeline-execution-engine.md) | 019-pipeline-config | 001-pipeline-model | Ready |
| 003-advanced-triggers | [Advanced Triggers](./003-advanced-triggers.md) | 019-pipeline-config | 001-pipeline-model, 002-pipeline-execution-engine | Ready |
| 004-pipeline-monitoring | [Pipeline Monitoring](./004-pipeline-monitoring.md) | 019-pipeline-config | 001-pipeline-model, 002-pipeline-execution-engine, 003-advanced-triggers | Ready |
| 005-howdy-project-setup | [Howdy project setup (Cargo + deps + layout)](./005-howdy-project-setup.md) | 019bd8-loop-spec-028-project-setup | None | Ready |
| 006-howdy-library-print-greeting | [Implement howdy::print_greeting (colored + error handling)](./006-howdy-library-print-greeting.md) | 019bd8-loop-spec-029-library-implementation | 005-howdy-project-setup | Ready |
| 007-howdy-cli-wireup | [Implement howdy CLI (clap args + error handling)](./007-howdy-cli-wireup.md) | 019bd8-loop-spec-030-cli-implementation | 005-howdy-project-setup, 006-howdy-library-print-greeting | Ready |
| 008-howdy-build-install | [Build release binary and install to ~/tmp/howdy](./008-howdy-build-install.md) | 019bd8-loop-spec-031-build-installation | 007-howdy-cli-wireup | Ready |
| 009-howdy-e2e-validation | [End-to-end validation scripts for howdy](./009-howdy-e2e-validation.md) | 019bd8-loop-spec-032-testing-validation | 008-howdy-build-install | Ready |
