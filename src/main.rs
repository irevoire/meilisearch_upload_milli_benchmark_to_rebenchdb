use rebenchdb_client::{
    Benchmark, BenchmarkData, Client, Criterion, DataPoint, Environment, Executor, Measure, Run,
    RunDetails, RunId, Source, Suite,
};

fn main() {
    let filename = std::env::args().nth(1).unwrap();
    let criterion = ureq::get(&format!(
        "https://milli-benchmarks.fra1.digitaloceanspaces.com/critcmp_results/{}",
        filename
    ))
    .call()
    .unwrap();
    let criterion: serde_json::Value = criterion.into_json().unwrap();

    // Setup everything
    let env = Environment {
        hostname: None,
        cpu: String::from("Bench"),
        clock_speed: 0,
        memory: 1024 * 1024 * 4, // 4GiB of ram
        os_type: String::from("Linux"),
        software: Vec::new(),
        user_name: String::from("Bench"),
        manual_run: false,
    };

    // Prepare to send the run to rebenchDB
    let client = Client::new("http://localhost:33333");

    let benchmark_data = handle_criterion_result(env.clone(), criterion);
    println!("{}", serde_json::to_string_pretty(&benchmark_data).unwrap());
    client.upload_results(benchmark_data).unwrap();
}

fn handle_criterion_result(env: Environment, criterion: serde_json::Value) -> BenchmarkData {
    // This field looks like that: `search_songs_main_6bf9824f`
    let benchmark_name = criterion["name"].as_str().unwrap();
    let (benchmark_name, commit) = benchmark_name.rsplit_once('_').unwrap();
    let (benchmark_name, branch) = benchmark_name.rsplit_once('_').unwrap();

    let (source, time) = Source::from_remote_repo_with_time(
        "http://github.com/meilisearch/meilisearch",
        branch,
        commit,
    )
    .unwrap();
    dbg!(&source);

    let mut benchmark_data = BenchmarkData::new(env, source, benchmark_name, time);
    benchmark_data.with_project("Milli's benchmark");

    for (sub_benchmark_name, benchmark) in criterion["benchmarks"].as_object().unwrap() {
        let bench = Benchmark {
            name: sub_benchmark_name.to_string(),
            suite: Suite {
                name: benchmark["fullname"].as_str().unwrap().to_string(),
                desc: None,
                executor: Executor {
                    name: String::from("Bench"),
                    desc: None,
                },
            },
            run_details: RunDetails {
                max_invocation_time: 0,
                min_iteration_time: 0,
                warmup: None,
            },
            desc: None,
        };

        dbg!(benchmark);

        let run_id = RunId {
            benchmark: bench,
            cmdline: format!(
                "cargo bench --bench {} -- {}",
                benchmark_name, sub_benchmark_name
            ),
            location: benchmark["criterion_benchmark_v1"]["directory_name"]
                .as_str()
                .unwrap()
                .to_string(),
            var_value: benchmark["criterion_benchmark_v1"]["value_str"]
                .as_str()
                .map(|s| s.to_string()),
            cores: None,
            input_size: None,
            extra_args: None,
        };

        let mut run = Run::new(run_id);

        let mut point = DataPoint::new(1, 10);

        // We lost all the infos about the real points so we're instead going to simulate the standard deviation around the median
        let median = &benchmark["criterion_estimates_v1"]["median"];
        let point_estimate = median["point_estimate"].as_f64().unwrap();
        let std_error = median["standard_error"].as_f64().unwrap();

        let simulate_n_points = 10;
        for _ in -simulate_n_points..simulate_n_points {
            point.add_point(Measure {
                criterion_id: 0,
                value: point_estimate + std_error / simulate_n_points as f64,
            })
        }

        run.add_data(point);

        benchmark_data.register_run(
            run,
            Criterion {
                id: 0,
                name: String::from("total"),
                unit: String::from("ms"),
            },
        );
    }

    benchmark_data
}