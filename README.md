# MEM012

mem012 is a CLI memory system for all Agents. It provides persistent memory and RAG retrieval, with a Web interface for managing memories.

<img src="frontend/public/mem012-architecture-en.png" alt="MEM012 architecture" width="600">

## Give Every Agent Long-Term Memory

- **Compatible with all Agents**: Supports the Agents mentioned above and any other runtime that can call CLI or HTTP/JSON tools, with no dedicated SDK required.
- **Persistent memory + RAG retrieval**: Stores memories in PostgreSQL and supports keyword, vector, and reranked retrieval.
- **Isolated memory**: Each Agent can have its own memory space, preventing context from leaking between Agents.
- **Visual memory review**: Agents make the calls, while the Web interface handles review. You can view memories and change records in one place, then approve, reject, restore, and maintain them.

## 1. Configure `config.toml`

```bash
cp config.example.toml config.toml
```

## 2. PostgreSQL

```bash
docker build -t mem012-postgres:pg18 -f docker/postgres/Dockerfile docker/postgres
```

```bash
export MEM012_ADMIN_POSTGRES_USER='mem012_admin'
export MEM012_POSTGRES_PASSWORD='your_admin_password'

docker run -d \
  --name mem012-postgres \
  --restart unless-stopped \
  --network 1panel-network \
  -p 5632:5432 \
  -e POSTGRES_USER="$MEM012_ADMIN_POSTGRES_USER" \
  -e POSTGRES_PASSWORD="$MEM012_POSTGRES_PASSWORD" \
  -v mem012_pg18_data:/var/lib/postgresql \
  mem012-postgres:pg18
```

## 3. Build and Install

Use the top-level `install.sh` for installation. The frontend is published to `/opt/1panel/www/sites/mem012/index` by default. On Linux systems using systemd, installing the backend also installs and starts `mem012.service` automatically.

```bash
# Install both the frontend and backend
sh install.sh

# Install only the frontend to the default directory
sh install.sh --frontend

# Install only the frontend to a specified directory (must be an absolute path)
sh install.sh --frontend /opt/1panel/www/sites/custom-site

# Install only the backend
sh install.sh --backend
```

## 4. Create a Profile

Each Agent can have its own profile to keep memories isolated.

For example, create a profile for Codex. This also creates the `mem_codex` database:

```bash
export MEM012_ADMIN_DATABASE_URL="postgresql://${MEM012_ADMIN_POSTGRES_USER}:${MEM012_POSTGRES_PASSWORD}@127.0.0.1:5632/postgres"
mem012 --create_profile codex
```

Creating a profile updates `config.toml`. Restart the service to load the new configuration and confirm that it is still running:

```bash
sudo systemctl restart mem012.service
sudo systemctl is-active --quiet mem012.service
```

## 5. Set Up Initial Memory (Optional)

1. Run `sudo systemctl is-active --quiet mem012.service` to confirm that the service is running. Then open `http://127.0.0.1:37777/auth` to obtain an `auth_token` valid for 5 minutes.
2. In the same user environment, run `mem012 --profile <profile> --auth <auth_token>` to generate the temporary authorization file `~/.auth/auth_file.mem`.
3. Use `create_memory` to create a memory in the `init` category. It will be read during initialization.

## 6. `SOUL.md`

Add the following block to your global instruction file.

```text
## INIT
Initialization is triggered only during the first conversation or after context compression. Do not run it again at other times.
My profile: codex.
mem012 is my memory system. After startup, I must first run the shell command `mem012 --profile codex init`, read the complete output, finish initialization, and only then continue processing the user's request.
```

## 7. SKILL && mem012_prompt

[SKILL.md](SKILL.md)

[mem012_prompt.md](mem012_prompt.md)

## 8. Memory Export/Import

Export memories:

```shell
mem012 --profile maccodex --args '{"tool":"backup_memory","params":{"output_path":"/absolute/path/backup.json"}}'
```

Import memories:

```shell
mem012 --profile target-profile --args '{"tool":"import_memory","params":{"input_path":"/absolute/path/backup.json"}}'
```
