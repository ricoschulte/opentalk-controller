# Metrics


## Web-API
All with labels {method, handler, status}

| Key                                       | Type      | Description                                                     |
| ----------------------------------------- | --------- | --------------------------------------------------------------- |
| web.request_durations                     | histogram | summary of request durations                                    |
| web.response_sizes                        | histogram | summary of response sizes                                       |
| sql.dbpool_connections                    | gauge     | Number of currently non-idling db connections                   |
| sql.dbpool_connections_idle               | gauge     | Number of currently idling db connections                       |
| sql.execution_time_seconds                | histogram | SQL query execution time for whole queries during web operation |
| sql.errors_total                          | counter   | Counter of SQL errors                                           |
| redis.execution_time_seconds              | histogram | Redis command execution time                                    |
| kustos.enforce_execution_time_seconds     | histogram | Kustos enforce execution time                                   |
| kustos.load_policy_execution_time_seconds | histogram | Kustos load policy execution time                               |
