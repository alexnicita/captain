# Private Zone (Not for git)

Use this folder for anything that should stay private on your machine.

Guiding principle: **personal = private**.

## Rules

- Put cloned private repos under: `private/repos/`
- Put secrets/notes under: `private/secrets/` and `private/notes/`
- Do **not** move private project files into tracked folders.

`private/*` is gitignored by default. Only this README and `.gitkeep` are tracked.

## Quick examples

```bash
# clone a private repo safely
mkdir -p private/repos
git clone git@github.com:<you>/<private-repo>.git private/repos/<private-repo>

# store a local secret note
mkdir -p private/notes
nano private/notes/local.md
```

## Agent instruction

When cloning repos by default:
- private/confidential repos → `private/repos/`
- public harness/framework contributions → tracked repo folders (for example `harnesses/`)
