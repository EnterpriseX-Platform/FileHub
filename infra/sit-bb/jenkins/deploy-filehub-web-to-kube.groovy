// Deploy job: deploy-filehub-web-to-kube — same template as the api deploy,
// pointed at the web Deployment (neb-filehub-app, NodePort 30814).
pipeline {
    agent any

    environment {
        KUBECONFIG_CREDENTIAL_ID = "${params.KUBECONFIG_CREDENTIAL_ID}"
        DEPLOYMENT = 'neb-filehub-app'
    }

    parameters {
        string(name: 'APP_NAME', defaultValue: 'neb-filehub-app', description: 'Application name to deploy')
        string(name: 'DOCKER_IMAGE', defaultValue: 'avalantglobal/filehub-web:latest', description: 'Docker image to deploy')
        string(name: 'KUBE_NAMESPACE', defaultValue: 'neb-dev', description: 'K8s namespace')
        string(name: 'KUBECONFIG_CREDENTIAL_ID', defaultValue: 'demo-neb-page-kubeconfig', description: 'commercial2 = neb-page-kubeconfig, center = demo-neb-page-kubeconfig')
    }

    stages {
        stage('Update Web Image') {
            steps {
                withCredentials([file(credentialsId: "${KUBECONFIG_CREDENTIAL_ID}", variable: 'KUBECONFIG')]) {
                    sh """
                    kubectl -n ${params.KUBE_NAMESPACE} set image deployment/${DEPLOYMENT} \
                        neb-filehub-app=${params.DOCKER_IMAGE}
                    """
                }
            }
        }

        stage('Verify Deployment') {
            steps {
                script {
                    withCredentials([file(credentialsId: "${KUBECONFIG_CREDENTIAL_ID}", variable: 'KUBECONFIG')]) {
                        sh """
                        kubectl -n ${params.KUBE_NAMESPACE} rollout status deployment/${DEPLOYMENT}
                        """
                    }
                }
            }
        }

        stage('Checking Status') {
            steps {
                script {
                    withCredentials([file(credentialsId: "${KUBECONFIG_CREDENTIAL_ID}", variable: 'KUBECONFIG')]) {
                        sh 'kubectl get pod -n ${KUBE_NAMESPACE} | grep ${DEPLOYMENT}'
                        echo "Sleep and wait for check again ..."
                        sh 'sleep 10'
                        sh 'kubectl get pod -n ${KUBE_NAMESPACE} | grep ${DEPLOYMENT}'
                    }
                }
            }
        }
    }
}
