SELECT category, SUM(score) FROM scores GROUP BY category HAVING SUM(score) >= -12
