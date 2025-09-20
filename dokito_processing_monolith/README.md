# Dokito Processing Monolith

A Rust-based microservice for efficiently processing government documents at scale. This system ingests raw legal case data, transforms it into standardized formats, downloads and processes attachments, and stores everything in AWS S3-compatible storage.

## Quick Start

### Prerequisites
- Docker and Docker Compose
- S3-compatible storage (AWS S3, DigitalOcean Spaces, etc.)
- DeepInfra API key (for LLM processing)

### Run with Docker

1. **Create environment file**:
```bash
cat > .env << EOF
DIGITALOCEAN_S3_ENDPOINT=https://nyc3.digitaloceanspaces.com
DIGITALOCEAN_S3_ACCESS_KEY_ID=your_access_key
DIGITALOCEAN_S3_SECRET_ACCESS_KEY=your_secret_key
OPENSCRAPERS_S3_OBJECT_BUCKET=your_bucket_name
DEEPINFRA_API_KEY=your_deepinfra_key
PUBLIC_SAFE_MODE=true
PORT=8123
EOF
```

2. **Build and run**:
```bash
docker build -t dokito-processing .
docker run -d --env-file .env -p 8123:8123 dokito-processing
```

3. **Verify it's running**:
```bash
curl http://localhost:8123/health
```

## System Overview

### Architecture
```
Raw Data → API Ingestion → Background Processing → S3 Storage → Public API Access
    ↓              ↓                    ↓              ↓            ↓
Legal Cases → JSON Validation → Data Transformation → objects/ → GET endpoints
Attachments → File Downloads → Hash Calculation → raw/file/ → File serving
```

### Key Features
- **Scalable Document Processing**: Handles legal cases with filings and attachments
- **Background Workers**: Asynchronous processing using task queues
- **S3 Integration**: Stores both raw and processed data
- **REST API**: Comprehensive API with auto-generated OpenAPI documentation
- **Safe Mode**: Production-safe mode that disables admin operations
- **Concurrent Processing**: Optimized for high-throughput document processing

## API Examples

### Submit a Case
```bash
curl -X POST http://localhost:8123/admin/cases/submit \
  -H "Content-Type: application/json" \
  -d '{
    "docket": {
      "case_govid": "PUC-2024-001",
      "case_name": "Sample Case",
      "case_url": "https://puc.ca.gov/cases/PUC-2024-001",
      "case_type": "Rate Review",
      "case_subtype": "Electricity",
      "description": "Sample rate review case",
      "industry": "Utilities",
      "petitioner": "Pacific Gas & Electric",
      "hearing_officer": "Commissioner Smith",
      "filings": [],
      "case_parties": [],
      "extra_metadata": {},
      "indexed_at": "2024-01-15T10:00:00Z"
    },
    "jurisdiction": {
      "country": "usa",
      "state": "ca",
      "jurisdiction": "puc"
    }
  }'
```

### Retrieve a Case
```bash
curl http://localhost:8123/public/cases/ca/puc/PUC-2024-001
```

### List Cases
```bash
curl "http://localhost:8123/public/caselist/ca/puc/all?limit=10&offset=0"
```

## Data Processing Pipeline

### Flow
1. **Raw Data Ingestion**: Submit cases via API or automated scrapers
2. **Validation**: Schema validation and data type checking
3. **Background Processing**: Transform raw → processed using trait-based system
4. **Attachment Processing**: Download files, calculate hashes, store in S3
5. **Storage**: Store processed data with indexed structure
6. **API Access**: Serve data via public REST endpoints

### Storage Structure
- **Raw Cases**: `objects_raw/{country}/{state}/{jurisdiction}/{case_name}.json`
- **Processed Cases**: `objects/{country}/{state}/{jurisdiction}/{case_name}.json`
- **Attachment Metadata**: `raw/metadata/{blake2b_hash}.json`
- **Attachment Files**: `raw/file/{blake2b_hash}`

## Environment Variables

### Required
```bash
DIGITALOCEAN_S3_ENDPOINT=https://nyc3.digitaloceanspaces.com
DIGITALOCEAN_S3_ACCESS_KEY_ID=your_access_key
DIGITALOCEAN_S3_SECRET_ACCESS_KEY=your_secret_key
OPENSCRAPERS_S3_OBJECT_BUCKET=your_bucket_name
DEEPINFRA_API_KEY=your_deepinfra_key
```

### Optional
```bash
PORT=8123                          # Server port (default: 8123)
PUBLIC_SAFE_MODE=true             # Disable admin routes (default: false)
RUST_LOG=info                     # Log level
PARENT_UT_POSTGRES_CONNECTION_STRING=postgresql://...  # For SQL tasks
```

## Development

### Prerequisites
- Rust 1.88+
- S3-compatible storage for development
- Optional: PostgreSQL for SQL ingestion tasks

### Setup
```bash
# Clone and build
git clone <repository-url>
cd dokito_processing_monolith
cargo build

# Set up development environment
cp .env.example .env.dev
# Edit .env.dev with your development values

# Run with development settings
export $(cat .env.dev | xargs) && cargo run

# Run tests
cargo test

# Format and lint
cargo fmt
cargo clippy
```

### Development Tools
```bash
# Auto-reload during development
cargo install cargo-watch
export $(cat .env.dev | xargs) && cargo watch -x run

# Advanced testing
cargo install cargo-nextest
cargo nextest run
```

## Documentation

Comprehensive documentation is available in the `docs/` directory:

- **[Usage Guide](docs/usage-guide.md)**: Complete system usage instructions
- **[API Examples](docs/api-examples.md)**: Detailed API usage with examples
- **[Processing Pipeline](docs/processing-pipeline.md)**: In-depth processing workflow
- **[Deployment Guide](docs/deployment-guide.md)**: Production deployment instructions
- **[Development Guide](docs/development-guide.md)**: Development setup and testing

### Additional Documentation
- **[Architecture Overview](docs/architecture-overview.md)**: High-level system design
- **[Data Types Pipeline](docs/data-types-pipeline.md)**: Data structure documentation
- **[Routes API](docs/routes-api.md)**: API endpoint reference
- **[Traits Processing](docs/traits-processing.md)**: Processing trait implementations

## Production Deployment

### Docker Compose
```yaml
version: '3.8'
services:
  dokito-processing:
    build: .
    ports:
      - "8123:8123"
    environment:
      - PUBLIC_SAFE_MODE=true
      - DIGITALOCEAN_S3_ENDPOINT=${S3_ENDPOINT}
      - DIGITALOCEAN_S3_ACCESS_KEY_ID=${S3_ACCESS_KEY}
      - DIGITALOCEAN_S3_SECRET_ACCESS_KEY=${S3_SECRET_KEY}
      - OPENSCRAPERS_S3_OBJECT_BUCKET=${S3_BUCKET}
      - DEEPINFRA_API_KEY=${DEEPINFRA_KEY}
    restart: unless-stopped
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:8123/health"]
      interval: 30s
      timeout: 10s
      retries: 3
```

### Kubernetes
See [Deployment Guide](docs/deployment-guide.md) for complete Kubernetes manifests including:
- Deployments with resource limits and health checks
- Services and ingress configuration
- Horizontal Pod Autoscaler setup
- ConfigMaps and Secrets management

## Performance

### Concurrency Limits
- **Attachment Downloads**: 10 concurrent per case
- **Case Processing**: 5 concurrent filings per case
- **Background Tasks**: Configurable worker pool

### Optimization Features
- Intelligent caching to avoid reprocessing unchanged data
- Content-based deduplication using Blake2b hashes
- Streaming file processing to minimize memory usage
- Concurrent processing with structured concurrency

## Security

### Production Safety
- **Safe Mode**: Set `PUBLIC_SAFE_MODE=true` to disable admin routes
- **Input Validation**: Comprehensive JSON schema validation
- **Secure Storage**: All file access through authenticated S3 APIs
- **Network Security**: Configurable CORS and network policies

### Best Practices
- Run containers as non-root user
- Use least-privilege IAM policies for S3 access
- Rotate API keys regularly
- Monitor and audit all operations

## Monitoring and Observability

### Health Checks
- `GET /health`: Basic service health
- `GET /test/deepinfra`: LLM API connectivity

### Logging
- Structured JSON logging with configurable levels
- OpenTelemetry integration for distributed tracing
- Request/response correlation IDs

### Metrics
- Processing time measurements
- Success/failure rates
- Resource utilization tracking

## License

[Add your license information here]

## Contributing

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Make your changes following the development guide
4. Run tests and ensure code quality (`cargo test && cargo clippy && cargo fmt`)
5. Commit your changes (`git commit -m 'feat: add amazing feature'`)
6. Push to the branch (`git push origin feature/amazing-feature`)
7. Open a Pull Request

See [Development Guide](docs/development-guide.md) for detailed contribution guidelines.

## Support

For questions, issues, or contributions:
- Check existing issues in the repository
- Review the comprehensive documentation in `docs/`
- Follow the troubleshooting guides for common problems

---

Built with ❤️ in Rust for efficient government document processing at scale.