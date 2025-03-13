use anyhow::Result;
use engine_builder::llm::prompts::{get_dockerfile_error_user_prompt, DOCKERFILE_ERROR_SYSTEM_PROMPT};
use engine_builder::stages::dockerfile::extract_dockerfile;

#[test]
fn test_extract_dockerfile_with_dockerfile_tag() {
    let response = r#"Here's the updated Dockerfile:

```dockerfile
FROM ubuntu:20.04

RUN apt-get update && \
    apt-get install -y python3 python3-pip

WORKDIR /app
COPY . .

RUN pip3 install -r requirements.txt

CMD ["python3", "app.py"]
```

This should fix the issue."#;

    let result = extract_dockerfile(response).unwrap();
    let expected = r#"FROM ubuntu:20.04

RUN apt-get update && \
    apt-get install -y python3 python3-pip

WORKDIR /app
COPY . .

RUN pip3 install -r requirements.txt

CMD ["python3", "app.py"]"#;

    assert_eq!(result, expected);
}

#[test]
fn test_extract_dockerfile_with_generic_tag() {
    let response = r#"Here's the updated Dockerfile:

```
FROM ubuntu:20.04

RUN apt-get update && \
    apt-get install -y python3 python3-pip

WORKDIR /app
COPY . .

RUN pip3 install -r requirements.txt

CMD ["python3", "app.py"]
```

This should fix the issue."#;

    let result = extract_dockerfile(response).unwrap();
    let expected = r#"FROM ubuntu:20.04

RUN apt-get update && \
    apt-get install -y python3 python3-pip

WORKDIR /app
COPY . .

RUN pip3 install -r requirements.txt

CMD ["python3", "app.py"]"#;

    assert_eq!(result, expected);
}

#[test]
fn test_extract_dockerfile_without_tags() {
    let response = r#"FROM ubuntu:20.04

RUN apt-get update && \
    apt-get install -y python3 python3-pip

WORKDIR /app
COPY . .

RUN pip3 install -r requirements.txt

CMD ["python3", "app.py"]"#;

    let result = extract_dockerfile(response).unwrap();
    assert_eq!(result, response);
}

#[test]
fn test_dockerfile_error_user_prompt() {
    let problem_statement = "Create a Docker image for a Python web application";
    let dockerfile_content = "FROM ubuntu:20.04\nRUN pip install flask\nCMD [\"python\", \"app.py\"]";
    let error_message = "The command '/bin/sh -c pip install flask' returned a non-zero code: 127";

    let prompt = get_dockerfile_error_user_prompt(problem_statement, dockerfile_content, error_message);

    assert!(prompt.contains(problem_statement));
    assert!(prompt.contains(dockerfile_content));
    assert!(prompt.contains(error_message));
    assert!(prompt.contains("<problem>"));
    assert!(prompt.contains("<dockerfile>"));
    assert!(prompt.contains("<error>"));
    assert!(prompt.contains("Format your updated Dockerfile between ```dockerfile and ``` tags"));
}
