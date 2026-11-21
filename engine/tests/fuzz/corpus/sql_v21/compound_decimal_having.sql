SELECT id, score, SUM(score) FROM widgets GROUP BY id, score HAVING (SUM(score) >= -1.25e+1 OR SUM(score) = 1.2e1) AND NOT SUM(score) < 0e0 ORDER BY score DESC LIMIT 2
