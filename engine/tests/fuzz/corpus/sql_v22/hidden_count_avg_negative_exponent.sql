SELECT id, score, AVG(score) FROM widgets GROUP BY id, score HAVING COUNT(*) >= 2 OR AVG(score) <= -1.25e-2
