ALTER TABLE `sessions` ADD `side_chat_of` text;--> statement-breakpoint
-- The parent link lived only in `session_details.data` until this column
-- existed; copy it out so the daemon's cascade and `agent_launch_env` can
-- see a side chat without hydrating it. Ordinary tasks correctly land NULL.
UPDATE `sessions`
SET `side_chat_of` = (
    SELECT json_extract(`session_details`.`data`, '$.side_chat_of')
    FROM `session_details`
    WHERE `session_details`.`session_id` = `sessions`.`id`
);
