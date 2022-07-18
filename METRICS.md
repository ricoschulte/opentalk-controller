# Metrics


## Web-API

| Key                                       | Type      | Labels                  | Description                                                     |
| ----------------------------------------- | --------- | ----------------------- | --------------------------------------------------------------- |
| web.request_durations                     | histogram | method, handler, status | summary of request durations                                    |
| web.response_sizes                        | histogram | method, handler, status | summary of response sizes                                       |
| signaling.runner_startup_time_seconds     | histogram | successful              | Time the runner takes to initialize                             |
| signaling.runner_destroy_time_seconds     | histogram | successful              | Time the runner takes to stop                                   |
| signaling.destroyed_rooms_count           | counter   |                         | Number of destroyed rooms                                       |
| signaling.participants_count              | gauge     | participation_kind      | Number of participants                                          |
| sql.dbpool_connections                    | gauge     |                         | Number of currently non-idling db connections                   |
| sql.dbpool_connections_idle               | gauge     |                         | Number of currently idling db connections                       |
| sql.execution_time_seconds                | histogram |                         | SQL query execution time for whole queries during web operation |
| sql.errors_total                          | counter   |                         | Counter of SQL errors                                           |
| redis.command_execution_time_seconds      | histogram | command                 | Redis command execution time                                    |
| kustos.enforce_execution_time_seconds     | histogram |                         | Kustos enforce execution time                                   |
| kustos.load_policy_execution_time_seconds | histogram |                         | Kustos load policy execution time                               |
