# Agent HTTP API

This page documents the structure of the HTTP API used by agents to communicate
with the crater server.

The base URL for the Agent API is `/agent-api/`.

## Authentication

All the requests to the Agent API are restricted to authenticated clients only.
Each agent should have its own unique API token. To authenticate you need to
provide the `Authorization` HTTP header with the right token:

```
Authorization: token YOUR-AGENT-TOKEN
```

If authentication fails the API returns a `403 Unauthorized` status code.

## Available endpoints

All the endpoints return a JSON response with a 200 status code if the request
succeded.

### `GET /config`

This endpoint returns the generic configuration of this agent, assigned by the
crater server. This method should be called at least at the start of the agent,
and the response is tied to the API token.

Response fields:

* `agent-name`: the name assigned by the crater server to this agent
* `crater-config`: the JSON serialized content of the server's `config.toml`

```json
{
    "agent-name": "crater-1",
    "crater-config": {...}
}
```

### `GET /next-experiment`

This endpoint returns the next experiment this agent should run. The first time
this method is called the first queued experiment is assigned to the agent, and
its configuration is returned. The same configuration is returned for all the
following calls, until the agent sends the full experiment result to the crater
server.

Response fields:

* `name`: the unique name assigned to this experiment
* `crates`: a list of all the crates part of this experiment
* `toolchains`: a list of the toolchains used in this experiment
* `mode`: the experiment mode

If there is no experiment available at the moment this returns only `null`.

```json
{
    "name": "experiment-1",
    "crates": [
        {
            "Version": {
                "name":"lazy_static",
                "version":"0.2.11"
            }
        },
        {
            "Repo": {
                "url": "https://github.com/brson/hello-rs"
            }
        }
    ],
    "toolchains": [
        {
            "Dist": "stable"
        },
        {
            "Dist": "beta"
        }
    ],
    "mode": "BuildAndTest"
}
```

### `POST /complete-experiment`

This endpoint marks the experiment currently being run by the authenticated
agent as complete. The server will publish the report, notify the user and
assign a new experiment to the agent.

The endpoint replies with an `OK` plain message.
