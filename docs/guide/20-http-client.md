# HTTP Client

Write HTTP requests in `.http` or `.rest` files and send them from the editor: each request gets a
run icon in the gutter, and the response opens in the Services tool window. It works in every Zen IDE.

## Writing requests

A request is a request line, its headers, a blank line and its body. Requests in one file are
separated by a line that starts with `###`; the text after `###` names the request.

```http
### List the items
GET https://api.example.test/items?page=1
Accept: application/json

###
# @name create
POST https://api.example.test/items
Content-Type: application/json
Authorization: Bearer {{token}}

{"name": "green tea"}
```

- The method is `GET`, `POST`, `PUT`, `PATCH`, `DELETE`, `HEAD`, `OPTIONS`, `TRACE` or `CONNECT`; a
  line with only a URL is a `GET`. A URL without `http://` or `https://` gets `http://`, and a path
  such as `/items` takes the host of the `Host` header.
- A long URL can go on over the next lines, each indented and starting with `?` or `&`.
- Lines starting with `#` or `//` before the request line or among the headers are comments.
  `# @name create` names the request; `# @no-redirect` does not follow redirects; `# @no-proxy` connects directly in a trusted project, whatever Settings → HTTP Proxy says.
- A body line `< ./payload.json` is replaced by that file, relative to the request file; the file
  must be inside the project.
- Response handler scripts (`> {% … %}`) and `>> file` are recognised and highlighted but not run yet.

The editor highlights the file, folds each request (⌘− and ⌘=) and lists the requests in the
Structure tool window (⌘7).

## Variables and environments

`{{name}}` is replaced when the request is sent:

1. by a variable of the file, written on its own line before a request: `@host = localhost:8080`;
2. by a variable of the selected environment;
3. by `{{$uuid}}`, `{{$timestamp}}` (seconds since 1970) or `{{$randomInt}}` (0 to 999).

Environments are in `http-client.env.json`, next to the request file or in a folder above it inside
the project; values that should not be shared, such as passwords and tokens, go into
`http-client.private.env.json` beside it, whose values win. Keep the private file out of version
control. Private values are sent, but the history and the Services tool window show them as
`{{name}}`.

```json
{
  "$shared": { "version": "v1" },
  "dev": { "host": "localhost:8080" },
  "prod": { "host": "api.example.test" }
}
```

The variables of `$shared` are in every environment. Choose the environment in the header of the
Services tool window (**No Environment** by default). A request with a variable that has no value is
not sent: a notice names the variables.

## Sending a request and reading the response

Click the run icon (▶) next to a request line. The request is sent at once with the selected
environment, and the **Services** tool window (⌘8, also View → Tool Windows → Services) opens with
it. Nothing is sent in any other way: opening, editing or highlighting a file never connects anywhere.

The left of the tool window lists the requests you ran, newest first, with their status code; click
one to see it on the right: the request line, the status line, the headers, the body (JSON is
indented) and the time it took. Requests go over HTTP or HTTPS (certificates are checked against the
usual public roots), redirects to other `http` and `https` addresses are followed (without the
`Authorization` header when the server changes), and a request that takes more than a minute or
answers with more than 16 MiB stops.

The IDE keeps the last 50 responses of each project in its own data folder, not in the project, so
they come back when you reopen the project.

## AI agents

Agents list the requests, run one and read the responses with the `http.*` commands
([automation](../automation.md#http-client)), once you have turned on the MCP server in
Settings → Tools → MCP Server.
