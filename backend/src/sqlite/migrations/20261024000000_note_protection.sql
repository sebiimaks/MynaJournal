ALTER TABLE entries ADD COLUMN protection TEXT DEFAULT NULL;

CREATE TRIGGER protected_entry_insert
BEFORE INSERT ON entries
WHEN NEW.protection IS NOT NULL AND
    (COALESCE(NEW.content, '') != '' OR NEW.priority IS NOT NULL)
BEGIN
    SELECT RAISE(ABORT, 'Protected entries cannot contain plaintext private fields');
END;

CREATE TRIGGER protected_entry_update
BEFORE UPDATE ON entries
WHEN NEW.protection IS NOT NULL AND
    (COALESCE(NEW.content, '') != '' OR NEW.priority IS NOT NULL OR
     EXISTS(SELECT 1 FROM tags WHERE entry_id = NEW.id))
BEGIN
    SELECT RAISE(ABORT, 'Protected entries cannot contain plaintext private fields');
END;

CREATE TRIGGER protected_entry_tag_insert
BEFORE INSERT ON tags
WHEN EXISTS(SELECT 1 FROM entries WHERE id = NEW.entry_id AND protection IS NOT NULL)
BEGIN
    SELECT RAISE(ABORT, 'Protected entries cannot contain plaintext tags');
END;

CREATE TRIGGER protected_entry_tag_update
BEFORE UPDATE ON tags
WHEN EXISTS(SELECT 1 FROM entries WHERE id = NEW.entry_id AND protection IS NOT NULL)
BEGIN
    SELECT RAISE(ABORT, 'Protected entries cannot contain plaintext tags');
END;
