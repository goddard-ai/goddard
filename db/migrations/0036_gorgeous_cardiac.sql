ALTER TABLE `sessions` ADD `task_configuration` text;
--> statement-breakpoint
-- Preserve existing selected options before list-only readers use this column.
UPDATE sessions SET task_configuration = (
    SELECT json_object(
        'type', 'configuration',
        'configuration', json_object(
            'provider', sessions.provider,
            'options', json_object(
                'mode', COALESCE(json_extract(detail.data, '$.runtime_mode'), 'autoAcceptEdits'),
                'model', sessions.model,
                'reasoningEffort', sessions.reasoning_effort,
                'serviceTier', json_extract(detail.data, '$.service_tier'),
                'contextWindow', json_extract(detail.data, '$.context_window')
            )
        ),
        'auto_route', json(CASE WHEN json_extract(detail.data, '$.auto_route') THEN 'true' ELSE 'false' END),
        'route_decision', json_extract(detail.data, '$.route_decision')
    ) FROM session_details AS detail
    WHERE detail.session_id = sessions.id AND json_valid(detail.data)
) WHERE task_configuration IS NULL;
