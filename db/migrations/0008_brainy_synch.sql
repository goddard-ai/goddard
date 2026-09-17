ALTER TABLE `sessions` ADD `workspace` text;--> statement-breakpoint
-- The workspace lived only in `session_details.data` until this column
-- existed; copy it out so list rows show worktree state without hydrating.
-- Local workspaces are omitted from the JSON, so they correctly land NULL.
UPDATE `sessions`
SET `workspace` = (
    SELECT json_extract(`session_details`.`data`, '$.workspace')
    FROM `session_details`
    WHERE `session_details`.`session_id` = `sessions`.`id`
);
