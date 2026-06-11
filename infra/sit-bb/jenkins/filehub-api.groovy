// Build job: filehub-api — modeled on jenkins-templates/neb-bpp-service.groovy.
// FileHub is a MONOREPO (backend/ + frontend/ in one repo), so the docker
// build runs with -f backend/Dockerfile against the backend/ context.
// Adjust GIT_REPO path if the repo lands somewhere other than bb/core.
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
                        if (fileExists('backend/Dockerfile')) {
                            docker.withRegistry('', "${DOCKER_CREDENTIALS_ID}") {
                                // Cluster nodes are amd64 — pin the platform so an
                                // ARM agent can't produce an unrunnable image.
                                sh 'docker build --platform=linux/amd64 -t ${DOCKER_IMAGE_NAME}/filehub-api:${VERSION} -f backend/Dockerfile backend/'
                                sh 'docker push ${DOCKER_IMAGE_NAME}/filehub-api:${VERSION}'
                                sh 'docker rmi ${DOCKER_IMAGE_NAME}/filehub-api:${VERSION}'
                            }
                            build job: 'deploy-filehub-api-to-kube', parameters: [
                                string(name: 'APP_NAME', value: 'neb-filehub-api'),
                                string(name: 'DOCKER_IMAGE', value: "${DOCKER_IMAGE_NAME}/filehub-api:${VERSION}"),
                                string(name: 'KUBE_NAMESPACE', value: "${KUBE_NAMESPACE}"),
                                string(name: 'KUBECONFIG_CREDENTIAL_ID', value: "${KUBECONFIG_CREDENTIAL_ID}")
                            ]
                        } else {
                            echo "backend/Dockerfile not found, skipping build."
                        }
                    }
                }
            }
        }
    }

    post {
        changed { echo "There were changes in this build for ${PROJECT} (api)" }
        success { echo "Build completed successfully for ${PROJECT} (api)" }
        failure { echo "Build failed for ${PROJECT} (api)" }
    }
}
