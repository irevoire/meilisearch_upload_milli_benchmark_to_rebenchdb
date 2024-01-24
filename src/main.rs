use rayon::iter::{ParallelBridge, ParallelIterator};
use rebenchdb_client::{
    Benchmark, BenchmarkData, Client, Criterion, DataPoint, Environment, Executor, Measure, Run,
    RunDetails, RunId, Source, Suite,
};

const CACHED_MEILI_REPO: &str = "/tmp/rebenchdb-meilisearch-repo";
const CACHED_MILLI_REPO: &str = "/tmp/rebenchdb-milli-repo";

fn main() {
    let benchmarks = include_str!("../benchmarks");
    std::fs::create_dir_all(CACHED_MEILI_REPO).unwrap();
    std::fs::create_dir_all(CACHED_MILLI_REPO).unwrap();

    let op = || {
        benchmarks.lines().par_bridge().for_each(|filename| {
            if filename.is_empty() {
                return;
            }
            let criterion = ureq::get(&format!(
                "https://milli-benchmarks.fra1.digitaloceanspaces.com/critcmp_results/{}",
                filename
            ))
            .call()
            .unwrap();
            let criterion: serde_json::Value = criterion.into_json().unwrap();

            let env = include_bytes!("../env.json");
            let env: Environment = serde_json::from_slice(env).unwrap();

            // Prepare to send the run to rebenchDB
            let client = Client::new("http://localhost:33333");

            let benchmark_data = match handle_criterion_result(env.clone(), criterion) {
                Ok(ret) => ret,
                Err(e) => {
                    eprintln!("{e} on {filename}");
                    return;
                }
            };
            println!("Sending {filename}");
            client
                .upload_results(benchmark_data)
                .expect("Make sure your rebenchDB server is up");
        })
    };

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(100)
        .build()
        .unwrap();
    pool.install(op);
}

fn handle_criterion_result(
    env: Environment,
    criterion: serde_json::Value,
) -> Result<BenchmarkData, git2::Error> {
    // This field looks like that: `search_songs_main_6bf9824f`
    let benchmark_name = criterion["name"].as_str().unwrap();
    let (benchmark_name, commit) = benchmark_name.rsplit_once('_').unwrap();
    let (benchmark_name, branch) = benchmark_name.rsplit_once('_').unwrap();

    let (source, time) = match Source::from_remote_repo_with_rev(
        "http://github.com/meilisearch/meilisearch",
        branch,
        commit,
        CACHED_MEILI_REPO,
    ) {
        Ok(ret) => ret,
        Err(e) if e.code() == git2::ErrorCode::NotFound => {
            eprintln!("Didn't find {commit} in meilisearch, looking in milli");
            Source::from_remote_repo_with_rev(
                "http://github.com/meilisearch/milli",
                branch,
                commit,
                CACHED_MILLI_REPO,
            )?
        }
        Err(e) => {
            return Err(e);
        }
    };

    let mut benchmark_data = BenchmarkData::new(env, source, benchmark_name, time);
    benchmark_data.with_project("Milli's benchmark");
    benchmark_data.push_criterion(Criterion {
        id: 0,
        name: String::from("total"),
        unit: String::from("ns"),
    });

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

        let mut point = DataPoint::new(1, 3);

        let median = &benchmark["criterion_estimates_v1"]["median"];
        let point_estimate = median["point_estimate"].as_f64().unwrap();
        let std_error = median["standard_error"].as_f64().unwrap();

        // We lost all the infos about the real points so we're instead going to simulate the standard deviation around the median by pushing three points
        point.add_point(Measure {
            criterion_id: 0,
            value: point_estimate - std_error,
        });
        point.add_point(Measure {
            criterion_id: 0,
            value: point_estimate,
        });
        point.add_point(Measure {
            criterion_id: 0,
            value: point_estimate + std_error,
        });
        run.add_data(point);

        benchmark_data.push_run(run);
    }

    Ok(benchmark_data)
}
