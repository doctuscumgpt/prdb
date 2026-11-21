SELECT id, score, SUM(score) FROM widgets GROUP BY id, score HAVING COUNT(*) >= 2 AND SUM(score) >= 4.50e0
