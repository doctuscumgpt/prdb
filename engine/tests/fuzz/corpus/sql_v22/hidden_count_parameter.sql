SELECT id, score, SUM(score) FROM widgets GROUP BY id, score HAVING COUNT(*) >= $1 AND SUM(score) >= 0.00
