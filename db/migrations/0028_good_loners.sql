ALTER TABLE `sessions` ADD `reasoning_effort` text;
--> statement-breakpoint
UPDATE sessions SET reasoning_effort = (
    SELECT CASE WHEN json_valid(data) THEN json_extract(data, '$.reasoning_effort') END
    FROM session_details WHERE session_id = sessions.id
);
