# kiwix-chat

A terminal chat UI where a **local LLM agent** answers questions using a **fully
offline Kiwix (Wikipedia) library**. The model runs an agentic tool-calling loop:
it searches your Kiwix server, reads the relevant articles, and answers with
citations — all without touching the internet.

```
┌──────────────┐   tool calls    ┌──────────────┐   HTTP    ┌────────────┐
│  ratatui TUI │◄──────────────► │ agent loop   │◄─────────►│ kiwix-serve│
└──────────────┘                 │ + LLM client │           └────────────┘
                                 └──────┬───────┘
                                   OpenAI /v1/chat/completions
                                 ┌──────▼───────┐
                                 │ llama.cpp /  │
                                 │ Ollama       │
                                 └──────────────┘
```

## Requirements

- Rust 1.85+
- A running [`kiwix-serve`](https://kiwix-tools.readthedocs.io/) with one or more ZIM files.
- A local OpenAI-compatible LLM server that supports tool/function calling:
  - **llama.cpp** (`llama-server`) on port `8080`, or
  - **Ollama** on port `11434`.

## Setup

### 1. Start Kiwix

```sh
# Note: llama.cpp also defaults to 8080, so run Kiwix on a different port.
kiwix-serve -p 8090 wikipedia_en_all_maxi_2024-01.zim
```

### 2. Start a local LLM (tool-calling capable model)

```sh
# llama.cpp
llama-server -m ./model.gguf --port 8080 --jinja

# or Ollama
ollama serve
ollama pull llama3.1   # a tool-calling model
```

### 3. Run kiwix-chat

```sh
cargo run --release -- --kiwix-url http://localhost:8090
```

On startup the app auto-detects the LLM backend (probing `:8080` then `:11434`,
3s timeout each) and uses the first advertised model.

## Usage

Type a question and press **Enter**. The agent will search Wikipedia, read
articles (shown as dim activity lines), and stream back a cited answer.

| Key / command      | Action                                  |
| ------------------ | --------------------------------------- |
| `Enter`            | Send the message                        |
| `PgUp` / `PgDn`    | Scroll the transcript                   |
| `/lang <code>`     | Set the search language (e.g. `/lang fra`) |
| `/clear`           | Clear the conversation                  |
| `/quit`            | Exit (also `Ctrl+C`)                    |

The status bar shows the active model, Kiwix reachability, and search language.

## Configuration

Every flag also reads an environment variable (flag > env > default/autodetect):

| Flag           | Env                   | Default                          | Description                              |
| -------------- | --------------------- | -------------------------------- | ---------------------------------------- |
| `--kiwix-url`  | `KIWIX_URL`           | `http://localhost:8080`          | kiwix-serve base URL (host + port + root)|
| `--llm-url`    | `KIWIX_CHAT_LLM_URL`  | autodetect `:8080` then `:11434` | OpenAI-compatible base URL               |
| `--model`      | `KIWIX_CHAT_MODEL`    | first `/v1/models` entry         | Model id                                 |
| `--lang`       | `KIWIX_CHAT_LANG`     | `eng`                            | 3-letter search language code            |
| `--max-rounds` | `KIWIX_CHAT_MAX_ROUNDS` | `6`                            | Max agent tool-call rounds per turn      |
| `--verbose`    |                       | off                              | Write `kiwix-chat.log` (TUI owns stdout) |

The `--kiwix-url` / `--llm-url` arguments accept a full base URL, so a remote
host, a non-default port, or a `--urlRootLocation` prefix all fit in one value
(e.g. `--kiwix-url http://192.168.1.50:8090/wiki`).

## Agent tools

The model is given three tools, backed by the public Kiwix HTTP API:

- **`search_wikipedia(query, lang?, limit?)`** → `/search?…&format=xml`
- **`read_article(zim_name, path)`** → `/raw/<zim>/content/<path>` (converted to text)
- **`list_books()`** → `/catalog/v2/entries` (discover available ZIMs & languages)

## Project layout

```
src/
  main.rs        # CLI, LLM autodetection, startup
  config.rs      # clap CLI + constants
  kiwix/         # kiwix-serve client + XML/OPDS parsing (client.rs, parse.rs)
  llm/           # OpenAI-compatible streaming client + types
  agent/         # tool schemas, dispatch, multi-round loop, system prompt
  tui/           # ratatui app state, event loop, rendering
```

## Notes

- Only offline, local endpoints are used; no external network calls are made.
- Answer quality depends on the local model's tool-calling ability and the ZIM
  content available to Kiwix.
