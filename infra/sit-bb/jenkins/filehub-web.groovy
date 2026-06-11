// Build job: filehub-web — Next.js frontend from the FileHub monorepo.
// The standalone bundle bakes the in-cluster BACKEND_URL at build time
// (see frontend/Dockerfile), so the build arg here must match the API
// Service name in infra/sit-bb/k8s/deployment-neb-filehub-api.yaml.
pipeline {
    agent any

    environment {
        GIT_REPO = 'http://gitadm:gitadmpassword@10.1.102.69/bb'
        DOCKER_CREDENTIALS_ID = 'docker-credential'
        PROJECT = 'filehub'
    }

    parameters {
        string(name: 'DOCKER_IMAGE_NAME', defaultValue: 'avalantglobal', description: 'Docker image namespace')
        string(name: 'VERSION', defaultValue: "v1.${new Date().format('yyyyMMdd.HHmm', TimeZone.getTimeZone('GMT+7'))}", description: 'Version of the image eg. V1.YYYYMMDD.HHMI')
        string(name: 'BACKEND_URL', defaultValue: 'http://neb-filehub-api:8090', description: 'In-cluster API service URL baked into the standalone bundle')
        choice(name: 'KUBE_NAMESPACE', choices: ['neb-dev'], description: 'K8s namespace')
        choice(name: 'KUBECONFIG_CREDENTIAL_ID', choices: ['demo-neb-page-kubeconfig','neb-page-kubeconfig'], description: 'Cert depend on namespace commercial2 = neb-page-kubeconfig, center = demo-neb-page-kubeconfig')
        string(name: 'GIT_REF', defaultValue: 'main', description: 'Git branch, tag, or commit to checkout (FileHub uses main, not develops)')
    }

    stages {
        stage('Clear file') {
            steps { script { sh 'if [ -f .git/index.lock ]; then rm -f .git/index.lock; fi' } }
        }
        stage('Clone Repositories') {
            steps {
                script {
                    dir("${PROJECT}") {
                        git branch: params.GIT_REF ?: 'main',
                            tags: true,
                            url: "${GIT_REPO}/core/${PROJECT}.git"
                    }
                }
            }
        }
        stage('Build Projects') {
            steps {
                script {
                    dir("${PROJECT}") {
                        if (fileExists('frontend/Dockerfile')) {
                            docker.withRegistry('', "${DOCKER_CREDENTIALS_ID}") {
                                sh 'docker build --platform=linux/amd64 --build-arg BACKEND_URL=${BACKEND_URL} -t ${DOCKER_IMAGE_NAME}/filehub-web:${VERSION} -f frontend/Dockerfile frontend/'
                                sh 'docker push ${DOCKER_IMAGE_NAME}/filehub-web:${VERSION}'
                                sh 'docker rmi ${DOCKER_IMAGE_NAME}/filehub-web:${VERSION}'
                            }
                            build job: 'deploy-filehub-web-to-kube', parameters: [
                                string(name: 'APP_NAME', value: 'neb-filehub-app'),
                                string(name: 'DOCKER_IMAGE', value: "${DOCKER_IMAGE_NAME}/filehub-web:${VERSION}"),
                                string(name: 'KUBE_NAMESPACE', value: "${KUBE_NAMESPACE}"),
                                string(name: 'KUBECONFIG_CREDENTIAL_ID', value: "${KUBECONFIG_CREDENTIAL_ID}")
                            ]
                        } else {
                            echo "frontend/Dockerfile not found, skipping build."
                        }
                    }
                }
            }
        }
    }

    post {
        changed { echo "There were changes in this build for ${PROJECT} (web)" }
        success { echo "Build completed successfully for ${PROJECT} (web)" }
        failure { echo "Build failed for ${PROJECT} (web)" }
    }
}
