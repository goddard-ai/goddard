CREATE TABLE `wireframe_discussions` (
	`id` text PRIMARY KEY NOT NULL,
	`task_id` text NOT NULL,
	`document_id` text NOT NULL,
	`path` text NOT NULL,
	`revision` text NOT NULL,
	`data` text NOT NULL
);
