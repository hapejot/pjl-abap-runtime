# Merge abapgit-inspect model files:
# jq -s -f abapgit-inspect/merge-models.jq model1.json model2.json > merged.json

def flatten_key($key):
  map(.[$key] // []) | add // [];

def relationship_fields:
  if has("fields") then
    . as $relationship
    | .fields[]
    | {
        left_table: $relationship.left_table,
        right_table: $relationship.right_table,
        left: .left,
        right: .right,
        sources: .sources
      }
  else
    {
      left_table,
      right_table,
      left: .left_field,
      right: .right_field,
      sources: [
        {source: (.source // "sql_equality"), file}
        + (if .line != null then {line} else {} end)
      ]
    }
  end;

def dependency_references:
  if has("references") then
    .references
  else
    [{file, line}]
  end;

if length == 0 then
  error("provide at least one model JSON")
elif (all(.[]; type == "object") | not) then
  error("each input must be a model JSON object")
else
  . as $models
  | {
      statements: (
        $models
        | flatten_key("statements")
        | unique_by([
            .file,
            .line,
            .operation,
            (.tables // []),
            (.fields // []),
            .target,
            .target_type
          ])
      ),
      tables: (
        $models
        | flatten_key("tables")
        | group_by(.name)
        | map({
            name: .[0].name,
            operations: ([.[] | (.operations // [])[]] | unique | sort),
            fields: ([.[] | (.fields // [])[]] | unique | sort)
          })
      ),
      relationships: (
        $models
        | flatten_key("relationships")
        | map(relationship_fields)
        | group_by([.left_table, .right_table])
        | map({
            left_table: .[0].left_table,
            right_table: .[0].right_table,
            fields: (
              group_by([.left, .right])
              | map({
                  left: .[0].left,
                  right: .[0].right,
                  sources: ([.[].sources[]] | unique_by([.source, .file, (.line // null)]))
                })
            )
          })
      ),
      structures: (
        $models
        | flatten_key("structures")
        | map(
            .fields = (
              (.fields // [])
              | unique_by([.name, .type])
              | sort_by(.name, .type)
            )
          )
        | unique_by([.name, .fields])
        | sort_by(.name, .fields)
      ),
      dependencies: (
        $models
        | flatten_key("dependencies")
        | map(
            . as $dependency
            | dependency_references
            | map(
                $dependency
                + {reference_file: .file, reference_line: .line}
              )
          )
        | add // []
        | group_by([
            .source_type,
            .source,
            (.target_type // null),
            .target,
            .kind
          ])
        | map({
            source_type: .[0].source_type,
            source: .[0].source,
            target_type: .[0].target_type,
            target: .[0].target,
            kind: .[0].kind,
            in_zip: ([.[].in_zip] | any),
            references: (
              map({file: .reference_file, line: .reference_line})
              | unique_by([.file, .line])
              | sort_by(.file, .line)
            )
          })
        | sort_by(.source_type, .source, .target, .kind)
      )
    }
end
