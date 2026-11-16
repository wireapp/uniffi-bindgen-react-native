{%- import "CallBodyMacros.ts" as cb %}
{%- macro function(func) %}
{%- call cb::docstring(func.docstring) %}
export {% if func.renders_async() %}async {% endif %}function {{ func.name }}(
    {%- call cb::arg_list_decl(func) -%}): {# space #}
    {%- call cb::return_type(func) %}
    {%- call cb::throws_kw(func) %} {
    {%- if func.sync_player %}
    // Excluded from async delivery: everything below, including lowering and
    // freeing buffers, uses the synchronous player on this thread.
    const nativeModule = syncNativeModule;
    {%- endif %}
    {%- call cb::call_body_function(func) %}
    }
{%- endmacro %}
