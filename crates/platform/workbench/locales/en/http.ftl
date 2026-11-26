http-error-not-http = { $path } is not an HTTP request file (.http or .rest).
http-error-which-request = The file has several requests: give the request's index or name.
http-error-no-request = There is no request { $request } in { $path }.
http-error-no-environment = There is no environment “{ $env }” in http-client.env.json or http-client.private.env.json.
http-error-env-file = Cannot read { $path }: { $error }
http-error-unresolved = Unresolved variables: { $names }. Define them in the file (@name = value) or in http-client.env.json, and select the environment.
http-error-body-file = Cannot read { $path } for the request body: { $error }
http-error-no-url = The request has no URL.
http-error-send = Cannot send the request: { $error }
http-error-unavailable = Requests cannot be sent here.
http-error-no-response = There is no response { $id }.
http-error-body-outside = { $path } is outside the project: a request body is read only from files inside it.
http-error-scheme = Only http and https requests are sent, not { $scheme }.
http-error-redirect-scheme = the server redirected to a { $scheme } address, which is not followed
http-error-redirects = the server redirected more than 10 times
http-error-timeout = no answer within a minute
