# Generate Markdown from an abapgit-inspect model or a merged model:
# jq -r -f abapgit-inspect/model-to-markdown.jq model.json > model.md
#
# Supports both flat and grouped relationship entries. The output describes
# inferred usage and relationships, not a complete database schema.
# Empty field lists mean that no fields were detected.

def code:
  "<code>\(@html | gsub("\\|"; "&#124;") | gsub("\n"; " "))</code>";

def code_list:
  if length == 0 then "-" else map(code) | join(", ") end;

def relationship_rows:
  . as $relationship
  | (if has("fields") then .fields[]
     else {left: .left_field, right: .right_field}
     end)
  | "| \($relationship.left_table | code) | \(.left | code) | \($relationship.right_table | code) | \(.right | code) |";

def dependency_reference_list:
  .references
  | map(if .line == null then .file else "\(.file):\(.line)" end | code)
  | join(", ");

def dependency_rows:
  . as $dependency
  | "| \(.source_type | code) | \(.source | code) | \((.target_type // "-") | code) | \(.target | code) | \(.kind | code) | \(.in_zip) | \($dependency | dependency_reference_list) |";

if type != "object" or
   (.tables | type) != "array" or
   (.relationships | type) != "array" or
   (.structures | type) != "array" or
   ((.dependencies // []) | type) != "array" then
  error("expected a model object with tables, relationships, structures and dependencies arrays")
else
  [
    "# Data model",
    "",
    "Automatically inferred usage and relationships; results may be incomplete or incorrect.",
    "",
    "## Tables",
    "",
    "| Table | Operations | Detected fields |",
    "| --- | --- | --- |",
    (.tables | sort_by(.name)[] |
      "| \(.name | code) | \(.operations | code_list) | \(.fields | code_list) |"),
    "",
    "## Relationships",
    "",
    "| Left table | Left field | Right table | Right field |",
    "| --- | --- | --- | --- |",
    (.relationships | sort_by(.left_table, .right_table)[] | relationship_rows),
    "",
    "## Structures",
    "",
    "| Structure | Field | Type |",
    "| --- | --- | --- |",
    (.structures | sort_by(.name)[] |
      .name as $name |
      if (.fields | length) == 0 then
        "| \($name | code) | - | - |"
      else
        .fields | sort_by(.name, .type)[] |
        "| \($name | code) | \(.name | code) | \(.type | code) |"
      end),
    "",
    "## Dependencies",
    "",
    "| Source type | Source | Target type | Target | Kind | In ZIP | References |",
    "| --- | --- | --- | --- | --- | --- | --- |",
    ((.dependencies // []) | sort_by(.source_type, .source, .target, .kind)[] | dependency_rows)
  ] | join("\n")
end
