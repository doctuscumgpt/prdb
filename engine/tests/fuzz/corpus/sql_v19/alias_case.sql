select L.label, R.value from lefts as L inner join rights as R on L.id = R.id and L.region = R.region
