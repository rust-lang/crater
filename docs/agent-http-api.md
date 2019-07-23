# Agent HTTP API

This page documents the structure of the HTTP API used by agents to communicate
with the crater server.

The base URL for the Agent API is `/agent-api/`.

## Authentication

All the requests to the Agent API are restricted to authenticated clients only.
Each agent should have its own unique API token. To authenticate you need to
provide the `Authorization` HTTP header with the right token:

```
Authorization: CraterToken YOUR-AGENT-TOKEN
```

If authentication fails the API returns a `403 Unauthorized` status code.

## Response format

Every valid endpoint of the Agent API returns a JSON payload as response. The
payload contains the following keys:

* `status`: the type of the response; can be `unauthorized`, `success`,
  `not-found` or `internal-error` (compatibility note: expect more types to be
  added in the future)
* `result`: the result of the request (only available if the status is `success`)
* `error`: the error message (only available if the status is `internal-error`)

```json
{
    "status": "success",
    "result": true
}
```

```json
{
    "status": "internal-error",
    "error": "Something happened"
}
```

## Expected behavior

While any endpoint can be called at any time, Crater expects a proper agent to
behave this way:

* `GET /config` should be called when the agent starts, and its results should
  be used as the configuration of the agent
* `POST /heartbeat` should be called when the agent starts and every minute,
  regardless of what the agent is doing
* `GET /agent-api/next-experiment` should be called when the agent is waiting
  for a new experiment; the endpoint returns `null` when there is no experiment
  available, so the agent should just call the endpoint again after a few
  seconds
* `POST /agent-api/record-progress` should be called as soon as a result is
  available
* `POST /error` should be called only when the agent has encountered an error

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
    "status": "success",
    "result": {
        "agent-name": "crater-1",
        "crater-config": {...}
    }
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

```json
{
    "status": "success",
    "result": {
        "name": "experiment-1",
        "crates": [
            {
                "Registry": {
                    "name":"lazy_static",
                    "version":"0.2.11"
                }
            },
            {
                "GitHub": {
                    "org": "brson",
                    "name": "hello-rs"
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
}
```

If there is no experiment, the result is `null`:

```json
{
    "status": "success",
    "result": null
}
```

### `POST /record-progress`

This endpoint uploads the result of a single job run by the agent to the Crater
server. The endpoint expects the following data to be provided as the request
body, encoded in JSON:

* `experiment-name`: the name of the experiment being run
* `results`: a list of job results that should be recorded:

    * `crate`: the serialized crate name
    * `toolchain`: the serialized toolchain name
    * `result`: the result of the experiment (for example `TestPass`)
    * `log`: the base64-encoded output of the job

* `shas`: a list of GitHub repo shas captured during the job; can be empty

For example, this is a valid request data:

```json
{
    "experiment-name": "pr-1",
    "results": [
        {
            "crate": {
                "GitHub": {
                    "org": "brson",
                    "repo": "hello-rs"
                }
            },
            "toolchain": {
                "Dist": "stable"
            },
            "result": "TestPass",
            "log": "cGlhZGluYSByb21hZ25vbGE="
        }
    ],
    "shas": [
        [
            {
                "org": "brson",
                "name": "hello-rs"
            },
            "f45e5e3289dd46aaec8392134a12c019aca3d117"
        ]
    ]
}
```

The endpoint replies with `true`.

```json
{
    "status": "success",
    "result": true
}
```

### `POST /heartbeat`

This endpoint tells the Crater server the agent is still alive. The method
should be called by the agent every minute, and after some time the method is
not called the Crater server will mark the agent as unreachable.

The endpoint replies with `true`.

```json
{
    "status": "success",
    "result": true
}
```

### `POST /error`

This endpoint tells the Crater server the agent has encountered an error.
The endpoint expects the error description to be provided as the request body,
encoded in JSON:

* `experiment-name`: the name of the experiment being run
* `error`: a description of the error

For example, this is a valid request data:

```json
{
    "experiment-name": "pr-1",
    "error": "pc is not powered on"
}
```

The endpoint replies with `true`.

```json
{
    "status": "success",
    "result": true
}
```
