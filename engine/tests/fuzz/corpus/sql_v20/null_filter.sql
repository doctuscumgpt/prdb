SELECT l.label, r.value FROM lefts AS l INNER JOIN rights AS r ON l.id = r.id AND l.region = r.region WHERE r.value IS NOT NULL;
