# Private Zone (Not for git)

Use this folder for anything that should stay private on your machine.

Guiding principle: **personal = private**.

## Rules

- Put cloned private repos under: `captain/private/repos/`
- Put secrets/notes under: `captain/private/secrets/` and `captain/private/notes/`
- Do **not** move private project files into tracked folders.

`captain/private/*` is gitignored by default. Only this README and `.gitkeep` are tracked.

## Quick examples

```bash
# clone a private repo safely
mkdir -p captain/private/repos
git clone git@github.com:<you>/<private-repo>.git captain/private/repos/<private-repo>

# store a local secret note
mkdir -p captain/private/notes
nano captain/private/notes/local.md
```

## Agent instruction

When cloning repos by default:
- captain/private/confidential repos → `captain/private/repos/`
- public harness/framework contributions → tracked repo folders (for example `captain/harnesses/`)
